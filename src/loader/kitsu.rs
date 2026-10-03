use super::Ctx;
use crate::db;
use crate::error::{log_error, log_info, log_warn, now_ts};
use crate::sources::kitsu::{self, Anime, ExternalIds};
use rusqlite::{named_params, params, Connection, OptionalExtension};
use std::time::Instant;

const SOURCE: &str = "kitsu";
const MAX_PAGES: u32 = 2_000;

/// Two passes: the first walks Kitsu's popularity order, which covers the
/// titles people actually look up. The second walks raw ids so long-tail
/// entries with no popularity data still get their Russian/metadata join.
const PASSES: &[&str] = &["-popularityRank", "ID"];

pub async fn run(ctx: Ctx) -> Result<(), String> {
    let per_page = ctx.cfg.sync_page_size.min(20); // Kitsu's comfortable max
    let t0 = Instant::now();

    match kitsu::fetch_genres(&ctx.sources.kitsu).await {
        Ok(v) => {
            if let Err(e) = with_conn(&ctx, |c| store_kitsu_genres(c, &v)) {
                log_warn(&format!("[kitsu] справочник жанров не сохранён: {}", e));
            }
        }
        Err(e) => log_warn(&format!("[kitsu] справочник жанров недоступен: {}", e)),
    }

    for sort in PASSES {
        if ctx.abort_requested() {
            log_info("[kitsu] прервано по запросу");
            return Ok(());
        }
        let task = format!("sort:{}", sort);
        let cp = with_conn(&ctx, |c| Ok(db::get_checkpoint(c, SOURCE, &task)))?;
        if cp.finished {
            log_info(&format!("[kitsu][{}] уже синхронизирован, пропускаю", sort));
            continue;
        }

        let mut page = (cp.last_page + 1).max(1) as u32;
        let mut failures = super::Failures::new();
        let mut last_reported = Instant::now();
        let mut total: u64;
        let mut saved = 0i64;

        while page <= MAX_PAGES {
            let (res, ids, meta_total) =
                match kitsu::fetch_page(&ctx.sources.kitsu, page, per_page, sort).await {
                    Ok(v) => v,
                    Err(e) => {
                        log_error(&format!("[kitsu][{}] стр. {}: {}", sort, page, e));
                        let _ = with_conn(&ctx, |c| {
                            db::mark_error(c, SOURCE, &task, &e);
                            Ok(())
                        });
                        match failures.record() {
                            super::OnError::NextPage => {
                                page += 1;
                                continue;
                            }
                            super::OnError::NextSort => {
                                log_error(&format!(
                                    "[kitsu][{}] {} ошибок подряд, следующая сортировка",
                                    sort,
                                    failures.streak()
                                ));
                                break;
                            }
                        }
                    }
                };
            failures.reset();
            total = meta_total;

            if res.data.is_empty() {
                with_conn(&ctx, |c| {
                    db::save_checkpoint(c, SOURCE, &task, page as i64, cp.total_saved + saved, true)
                })?;
                break;
            }

            let has_next = if meta_total > 0 {
                (page as u64).saturating_mul(per_page as u64) < meta_total
            } else {
                res.data.len() as u64 >= per_page as u64
            };

            let mut touched: Vec<String> = Vec::with_capacity(res.data.len());
            for (res, ext) in res.data.iter().zip(ids.iter()) {
                match with_conn(&ctx, |c| upsert(c, res, ext)) {
                    Ok(()) => {
                        saved += 1;
                        touched.push(target_uid_for(res, ext));
                    }
                    Err(e) => log_warn(&format!("[kitsu] id={}: {}", res.id, e)),
                }
            }
            with_conn(&ctx, |c| {
                db::save_checkpoint(
                    c,
                    SOURCE,
                    &task,
                    page as i64,
                    cp.total_saved + saved,
                    !has_next,
                )
            })?;

            // Kitsu contributes studio names, so re-link genres for the page.
            if !touched.is_empty() {
                let _ = with_conn(&ctx, |c| Ok(super::genres::match_uids(c, &touched)));
            }

            if !has_next || page % 20 == 0 || last_reported.elapsed().as_secs() >= 15 {
                last_reported = Instant::now();
                let in_db = with_conn(&ctx, |c| {
                    c.query_row(
                        "SELECT COUNT(*) FROM anime WHERE kitsu_id IS NOT NULL",
                        [],
                        |r| r.get::<_, i64>(0),
                    )
                })
                .unwrap_or(0);
                log_info(&format!(
                    "[kitsu][{}] стр. {}/{}: +{} (всего {}), привязано к каталогу: {}",
                    sort,
                    page,
                    (total as f64 / per_page as f64).ceil() as u64,
                    saved,
                    total,
                    in_db
                ));
            }

            if !has_next {
                break;
            }
            page += 1;
        }
    }

    log_info(&format!(
        "[kitsu] готово за {}",
        super::human_secs(t0.elapsed().as_secs())
    ));
    Ok(())
}

fn with_conn<T>(
    ctx: &Ctx,
    f: impl FnOnce(&Connection) -> Result<T, rusqlite::Error>,
) -> Result<T, String> {
    let c = ctx.db.conn().map_err(|e| e.to_string())?;
    f(&c).map_err(|e| {
        log_error(&format!("db: {}", e));
        e.to_string()
    })
}

/// Kitsu genres and categories become real rows in `genres` so the filter UI
/// can show Russian names alongside English ones.
fn store_kitsu_genres(conn: &Connection, value: &serde_json::Value) -> Result<(), rusqlite::Error> {
    let data = value
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| rusqlite::Error::InvalidParameterName("kitsu genres: нет data".into()))?;

    for g in data {
        let attrs = match g.get("attributes") {
            Some(a) => a,
            None => continue,
        };
        let name_en = attrs
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if name_en.is_empty() {
            continue;
        }
        // Kitsu's own slug is already a normalised key.
        let slug = attrs
            .get("slug")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| super::title_key(name_en));

        let is_category = attrs
            .get("dimension")
            .and_then(|v| v.as_str())
            .map(|d| d == "explicitly_requested")
            .unwrap_or(false);

        conn.execute(
            "INSERT INTO genres (slug, name_en, name_ru, category, created_at)
             VALUES (?1, ?2, NULL, ?3, ?4)
             ON CONFLICT(slug) DO UPDATE SET
                name_en  = COALESCE(genres.name_en, excluded.name_en),
                category = COALESCE(excluded.category, genres.category)",
            params![
                slug,
                name_en,
                if is_category { "category" } else { "genre" },
                now_ts()
            ],
        )?;
    }
    Ok(())
}

/// Russian names for the well-known genre slugs. Shikimori and AniList supply
/// the rest at match time; this table only covers the ones a user is most
/// likely to filter by, so the UI is not a wall of English words.
const GENRE_RU: &[(&str, &str)] = &[
    ("action", "Боевик"),
    ("adventure", "Приключения"),
    ("comedy", "Комедия"),
    ("drama", "Драма"),
    ("ecchi", "Этти"),
    ("fantasy", "Фэнтези"),
    ("hentai", "Хентай"),
    ("historical", "Исторический"),
    ("horror", "Ужасы"),
    ("kids", "Детский"),
    ("magic", "Магия"),
    ("mecha", "Меха"),
    ("music", "Музыка"),
    ("mystery", "Мистика"),
    ("psychological", "Психологический"),
    ("romance", "Романтика"),
    ("sci-fi", "Научная фантастика"),
    ("slice-of-life", "Повседневность"),
    ("sports", "Спорт"),
    ("supernatural", "Сверхъестественное"),
    ("thriller", "Триллер"),
    ("vampire", "Вампиры"),
    ("yuri", "Юри"),
    ("space", "Космос"),
    ("cyberpunk", "Киберпанк"),
    ("gore", "Жестокость"),
    ("military", "Военный"),
    ("police", "Полиция"),
    ("samurai", "Самурай"),
    ("award-winning", "Награды"),
    ("female-protagonist", "Главная героиня"),
];

fn apply_genre_ru(conn: &Connection) -> Result<(), rusqlite::Error> {
    for (slug, ru) in GENRE_RU {
        conn.execute(
            "UPDATE genres SET name_ru = ?1 WHERE slug = ?2 AND (name_ru IS NULL OR name_ru = '')",
            params![ru, slug],
        )?;
    }
    Ok(())
}

/// Merges one Kitsu row into the catalogue.
///
/// The interesting part is the identity decision. v1 had no way to do this, so
/// it invented a negative `anilist_id` for anything missing from AniList and
/// two sources could collide on the same number. Here the row is keyed by
/// AniList id when the `mappings` include one — so a Kitsu title that AniList
/// also knows simply enriches the existing row — and by Kitsu id otherwise.
/// The uid a row will end up under, mirroring the choice made in `upsert`.
fn target_uid_for(res: &kitsu::Resource<Anime>, ext: &ExternalIds) -> String {
    match ext.anilist_id {
        Some(al) => format!("al:{}", al),
        None => format!("ks:{}", res.id),
    }
}

fn upsert(
    conn: &Connection,
    res: &kitsu::Resource<Anime>,
    ext: &ExternalIds,
) -> Result<(), rusqlite::Error> {
    let kitsu_id: i64 = res
        .id
        .parse()
        .map_err(|_| rusqlite::Error::InvalidParameterName(format!("kitsu id {}", res.id)))?;
    let a = &res.attributes;

    let uid = match ext.anilist_id {
        Some(al) => format!("al:{}", al),
        None => format!("ks:{}", kitsu_id),
    };

    // If AniList already created the row, keep its uid; if we are creating the
    // `al:` row ourselves it is correct by construction.
    let existing_uid: Option<String> = conn
        .query_row(
            "SELECT uid FROM anime WHERE anilist_id = ?1 OR kitsu_id = ?2 LIMIT 1",
            params![ext.anilist_id, kitsu_id],
            |r| r.get(0),
        )
        .optional()?;

    let target_uid = existing_uid.unwrap_or_else(|| uid.clone());

    let title_en = a.titles.en_jp.clone().or_else(|| a.titles.en.clone());
    let title_native = a.titles.ja_jp.clone();
    let title_romaji = title_en.clone().or_else(|| a.canonical_title.clone());
    let key = title_romaji.as_deref().map(super::title_key);

    let mut alt: Vec<String> = Vec::new();
    for v in [
        a.titles.en.as_deref(),
        a.titles.en_jp.as_deref(),
        a.titles.en_us.as_deref(),
        a.titles.en_gb.as_deref(),
        a.titles.ja_jp.as_deref(),
        a.titles.pt_br.as_deref(),
        a.canonical_title.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        let v = v.trim();
        if !v.is_empty() && !alt.iter().any(|x| x == v) {
            alt.push(v.to_string());
        }
    }
    let alt_json = if alt.is_empty() {
        None
    } else {
        serde_json::to_string(&alt).ok()
    };

    // Kitsu reports a percentage already, so no /10 conversion is needed.
    let score: Option<i64> = a
        .average_rating
        .as_ref()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .map(|v| v.round() as i64);
    let score_source = score.map(|_| "kitsu");

    let format = a.subtype.as_ref().map(|s| normalize_format(s));
    let adult = a.nsfw.map(|v| if v { 1 } else { 0 }).unwrap_or(0);
    let classification = serde_json::json!({
        "ageRating": a.age_rating,
        "ageRatingGuide": a.age_rating_guide,
    })
    .to_string();

    let (sy, sm, sd) = split_iso(a.start_date.as_deref());
    let (ey, em, ed) = split_iso(a.end_date.as_deref());

    // Named parameters, for the same reason as the AniList upsert: a
    // positional list is easy to miscount when a column is added.
    conn.execute(
        r#"
        INSERT INTO anime (
            uid, anilist_id, kitsu_id, mal_id,
            title_romaji, title_english, title_native, title_key, alt_titles,
            format, status, description, duration, episodes, is_adult,
            start_date, end_date, start_year, start_month, start_day,
            end_year, end_month, end_day,
            score, score_source, popularity, favourites, rating_count,
            cover_small, cover_medium, cover_large, banner,
            trailer_id, trailer_site, classifications_json,
            created_at, updated_at, kitsu_synced_at
        ) VALUES (
            :uid, :anilist_id, :kitsu_id, :mal_id,
            :title_romaji, :title_english, :title_native, :title_key, :alt_titles,
            :format, :status, :description, :duration, :episodes, :is_adult,
            :start_date, :end_date, :start_year, :start_month, :start_day,
            :end_year, :end_month, :end_day,
            :score, :score_source, :popularity, :favourites, :rating_count,
            :cover_small, :cover_medium, :cover_large, :banner,
            :trailer_id, 'youtube', :classifications_json,
            :now, :now, :now
        )
        ON CONFLICT(uid) DO UPDATE SET
            anilist_id  = COALESCE(excluded.anilist_id, anime.anilist_id),
            kitsu_id    = COALESCE(excluded.kitsu_id, anime.kitsu_id),
            mal_id      = COALESCE(excluded.mal_id, anime.mal_id),
            -- Titles prefer whatever is already stored: AniList's naming is
            -- curated, so a Kitsu duplicate must not overwrite it.
            title_romaji  = COALESCE(anime.title_romaji, excluded.title_romaji),
            title_english = COALESCE(anime.title_english, excluded.title_english),
            title_native  = COALESCE(anime.title_native, excluded.title_native),
            title_key     = COALESCE(anime.title_key, excluded.title_key),
            alt_titles    = COALESCE(anime.alt_titles, excluded.alt_titles),
            format        = COALESCE(anime.format, excluded.format),
            status        = COALESCE(excluded.status, anime.status),
            description   = COALESCE(anime.description, excluded.description),
            duration      = COALESCE(excluded.duration, anime.duration),
            episodes      = COALESCE(excluded.episodes, anime.episodes),
            is_adult      = COALESCE(excluded.is_adult, anime.is_adult),
            start_date    = COALESCE(anime.start_date, excluded.start_date),
            end_date      = COALESCE(anime.end_date, excluded.end_date),
            start_year    = COALESCE(anime.start_year, excluded.start_year),
            start_month   = COALESCE(anime.start_month, excluded.start_month),
            start_day     = COALESCE(anime.start_day, excluded.start_day),
            end_year      = COALESCE(excluded.end_year, anime.end_year),
            end_month     = COALESCE(excluded.end_month, anime.end_month),
            end_day       = COALESCE(excluded.end_day, anime.end_day),
            score         = COALESCE(anime.score, excluded.score),
            score_source  = COALESCE(anime.score_source, excluded.score_source),
            popularity    = COALESCE(excluded.popularity, anime.popularity),
            favourites    = COALESCE(excluded.favourites, anime.favourites),
            rating_count  = COALESCE(excluded.rating_count, anime.rating_count),
            cover_small   = COALESCE(anime.cover_small, excluded.cover_small),
            cover_medium  = COALESCE(anime.cover_medium, excluded.cover_medium),
            cover_large   = COALESCE(anime.cover_large, excluded.cover_large),
            banner        = COALESCE(anime.banner, excluded.banner),
            trailer_id    = COALESCE(anime.trailer_id, excluded.trailer_id),
            trailer_site  = COALESCE(anime.trailer_site, excluded.trailer_site),
            classifications_json = COALESCE(excluded.classifications_json, anime.classifications_json),
            updated_at    = excluded.updated_at,
            kitsu_synced_at = excluded.kitsu_synced_at
        "#,
        named_params! {
            ":uid": target_uid,
            ":anilist_id": ext.anilist_id,
            ":kitsu_id": kitsu_id,
            ":mal_id": ext.mal_id,
            ":title_romaji": title_romaji,
            ":title_english": title_en,
            ":title_native": title_native,
            ":title_key": key,
            ":alt_titles": alt_json,
            ":format": format,
            ":status": a.status,
            ":description": a.synopsis.as_deref().map(super::strip_html),
            ":duration": a.episode_length,
            ":episodes": a.episode_count,
            ":is_adult": adult,
            ":start_date": a.start_date,
            ":end_date": a.end_date,
            ":start_year": sy,
            ":start_month": sm,
            ":start_day": sd,
            ":end_year": ey,
            ":end_month": em,
            ":end_day": ed,
            ":score": score,
            ":score_source": score_source,
            ":popularity": a.popularity_rank,
            ":favourites": a.favorites_count,
            ":rating_count": a.user_count,
            ":cover_small": a.poster_image.small,
            ":cover_medium": a.poster_image.medium,
            ":cover_large": a.poster_image.large,
            ":banner": a.cover_image.as_ref().and_then(|c| c.large.clone())
                        .or_else(|| a.poster_image.original.clone()),
            ":trailer_id": a.youtube_video_id,
            ":classifications_json": classification,
            ":now": now_ts()
        },
    )?;

    apply_genre_ru(conn)?;
    Ok(())
}

/// Kitsu vocabulary > AniList vocabulary, so a single `format` filter works
/// regardless of which source filled the column.
fn normalize_format(s: &str) -> String {
    match s.to_ascii_uppercase().as_str() {
        "TV" | "TV_SHORT" => "TV".into(),
        "MOVIE" | "FILM" => "MOVIE".into(),
        "OVA" => "OVA".into(),
        "ONA" => "ONA".into(),
        "SPECIAL" | "SPECIALS" => "SPECIAL".into(),
        "MUSIC" | "CLIP" => "MUSIC".into(),
        other => {
            let mut c = other.chars();
            match c.next() {
                Some(f) => f.to_string() + c.as_str(),
                None => other.to_string(),
            }
        }
    }
}

/// `YYYY-MM-DD` (any prefix may be missing) -> `(year, month, day)`.
fn split_iso(s: Option<&str>) -> (Option<i64>, Option<i64>, Option<i64>) {
    let Some(s) = s else {
        return (None, None, None);
    };
    let mut it = s.split('-');
    let y = it.next().and_then(|v| v.trim().parse().ok());
    let m = it.next().and_then(|v| v.trim().parse().ok());
    let d = it.next().and_then(|v| v.trim().parse().ok());
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::testing::{conn, insert_anime};
    use crate::sources::kitsu::{ExternalIds, ImageSet, Resource, Titles};
    use serde_json::json;

    fn resource(id: &str) -> Resource<Anime> {
        Resource {
            id: id.to_string(),
            attributes: Anime {
                canonical_title: Some("Cowboy Bebop".into()),
                titles: Titles {
                    en: Some("Cowboy Bebop".into()),
                    en_jp: Some("Cowboy Bebop".into()),
                    en_us: None,
                    en_gb: None,
                    ja_jp: Some("カウボーイビバップ".into()),
                    pt_br: None,
                },
                synopsis: Some("<p>A bounty hunter crew.</p>".into()),
                average_rating: Some("82.27".into()),
                user_count: Some(500_000),
                favorites_count: Some(20_000),
                popularity_rank: Some(42),
                age_rating: Some("R17+".into()),
                subtype: Some("TV".into()),
                status: Some("finished".into()),
                start_date: Some("1998-04-03".into()),
                end_date: None,
                episode_count: Some(26),
                episode_length: Some(24),
                youtube_video_id: Some("xyz".into()),
                nsfw: Some(false),
                slug: Some("cowboy-bebop".into()),
                poster_image: ImageSet {
                    small: Some("s.jpg".into()),
                    medium: Some("m.jpg".into()),
                    large: Some("l.jpg".into()),
                    ..ImageSet::default()
                },
                cover_image: Some(ImageSet {
                    large: Some("cover.jpg".into()),
                    ..ImageSet::default()
                }),
                ..Anime::default()
            },
            relationships: Default::default(),
        }
    }

    fn get_str(c: &Connection, uid: &str, col: &str) -> Option<String> {
        c.query_row(
            &format!("SELECT {} FROM anime WHERE uid = ?1", col),
            [uid],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn get_i64(c: &Connection, uid: &str, col: &str) -> Option<i64> {
        c.query_row(
            &format!("SELECT {} FROM anime WHERE uid = ?1", col),
            [uid],
            |r| r.get(0),
        )
        .unwrap()
    }

    // ------------------------------------------------------- format vocabulary

    #[test]
    fn kitsu_formats_are_folded_onto_the_shared_vocabulary() {
        // One `format` filter has to work no matter which source filled the
        // column, so Kitsu's spellings are mapped rather than stored raw.
        for (raw, want) in [
            ("TV", "TV"),
            ("tv", "TV"),
            ("TV_SHORT", "TV"),
            ("MOVIE", "MOVIE"),
            ("Film", "MOVIE"),
            ("OVA", "OVA"),
            ("ONA", "ONA"),
            ("SPECIAL", "SPECIAL"),
            ("specials", "SPECIAL"),
            ("MUSIC", "MUSIC"),
            ("CLIP", "MUSIC"),
        ] {
            assert_eq!(normalize_format(raw), want, "format {}", raw);
        }
    }

    #[test]
    fn an_unknown_kitsu_format_is_kept_uppercased() {
        // A format the filter sheet has never heard of must still be stored:
        // dropping it would make the title invisible under its own filter.
        assert_eq!(normalize_format("tv_special"), "TV_SPECIAL");
        assert_eq!(normalize_format("unknown"), "UNKNOWN");
    }

    #[test]
    fn normalize_format_never_panics_on_an_empty_string() {
        assert_eq!(normalize_format(""), "");
    }

    // --------------------------------------------------------- dates

    #[test]
    fn a_full_iso_date_splits_into_three_numbers() {
        assert_eq!(
            split_iso(Some("1998-04-03")),
            (Some(1998), Some(4), Some(3))
        );
    }

    #[test]
    fn a_partial_iso_date_keeps_what_it_has() {
        // Kitsu sends a year for a movie and a full date for a series, so
        // both shapes have to survive.
        assert_eq!(split_iso(Some("1998")), (Some(1998), None, None));
        assert_eq!(split_iso(Some("1998-04")), (Some(1998), Some(4), None));
    }

    #[test]
    fn a_missing_or_unparsable_date_is_all_nulls() {
        // Unparsable input becomes NULL rather than zero: a year 0 row would
        // sit inside every year range filter in the UI.
        assert_eq!(split_iso(None), (None, None, None));
        assert_eq!(split_iso(Some("")), (None, None, None));
        assert_eq!(split_iso(Some("неизвестно")), (None, None, None));
        // Each field is parsed on its own, so a broken month leaves the year
        // and the day usable. The raw string is stored separately, so nothing
        // is invented.
        assert_eq!(split_iso(Some("1998-AB-03")), (Some(1998), None, Some(3)));
    }

    // ---------------------------------------------------------- identity

    #[test]
    fn a_row_with_an_anilist_mapping_joins_the_anilist_row() {
        let ext = ExternalIds {
            anilist_id: Some(16498),
            ..ExternalIds::default()
        };
        assert_eq!(target_uid_for(&resource("12"), &ext), "al:16498");
    }

    #[test]
    fn a_row_without_an_anilist_mapping_gets_its_own_kitsu_uid() {
        // v1 invented a negative AniList id here, so a Kitsu-only title and an
        // AniList title could collide on one number.
        let ext = ExternalIds {
            anilist_id: None,
            ..ExternalIds::default()
        };
        assert_eq!(target_uid_for(&resource("12"), &ext), "ks:12");
    }

    #[test]
    fn a_kitsu_row_attaches_to_an_anilist_row_that_already_exists() {
        let c = conn();
        insert_anime(&c, "al:16498", Some("Shingeki no Kyojin"));
        let ext = ExternalIds {
            anilist_id: Some(16498),
            ..ExternalIds::default()
        };
        upsert(&c, &resource("12"), &ext).unwrap();

        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM anime", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "присоединение не должно создавать вторую строку");
        assert_eq!(get_i64(&c, "al:16498", "kitsu_id"), Some(12));
    }

    #[test]
    fn a_kitsu_row_joins_an_existing_row_found_by_its_own_kitsu_id() {
        // The mapping may be missing in the response, but if the row is already
        // in the catalogue under a kitsu_id, it must still be enriched.
        let c = conn();
        c.execute(
            "INSERT INTO anime (uid, kitsu_id, created_at) VALUES ('al:16498', 12, 1)",
            [],
        )
        .unwrap();
        upsert(&c, &resource("12"), &ExternalIds::default()).unwrap();
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM anime", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(get_i64(&c, "al:16498", "rating_count"), Some(500_000));
    }

    #[test]
    fn a_kitsu_only_title_lands_under_a_kitsu_uid() {
        let c = conn();
        upsert(&c, &resource("12"), &ExternalIds::default()).unwrap();
        let uid: String = c
            .query_row("SELECT uid FROM anime WHERE kitsu_id = 12", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(uid, "ks:12");
    }

    #[test]
    fn a_non_numeric_kitsu_id_is_an_error_not_a_panic() {
        let c = conn();
        let mut r = resource("not-a-number");
        r.id = "abc".into();
        let err = upsert(&c, &r, &ExternalIds::default());
        assert!(err.is_err());
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM anime", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    // ----------------------------------------------------------- mapping

    #[test]
    fn stores_the_kitsu_fields_under_our_own_column_names() {
        let c = conn();
        upsert(&c, &resource("12"), &ExternalIds::default()).unwrap();
        assert_eq!(
            get_str(&c, "ks:12", "title_romaji").as_deref(),
            Some("Cowboy Bebop")
        );
        assert_eq!(
            get_str(&c, "ks:12", "title_english").as_deref(),
            Some("Cowboy Bebop")
        );
        assert_eq!(
            get_str(&c, "ks:12", "title_native").as_deref(),
            Some("カウボーイビバップ")
        );
        assert_eq!(get_i64(&c, "ks:12", "episodes"), Some(26));
        assert_eq!(get_i64(&c, "ks:12", "duration"), Some(24));
        assert_eq!(get_i64(&c, "ks:12", "popularity"), Some(42));
        assert_eq!(get_i64(&c, "ks:12", "favourites"), Some(20_000));
        assert_eq!(get_i64(&c, "ks:12", "rating_count"), Some(500_000));
        assert_eq!(get_str(&c, "ks:12", "trailer_id").as_deref(), Some("xyz"));
        assert_eq!(
            get_str(&c, "ks:12", "trailer_site").as_deref(),
            Some("youtube")
        );
    }

    #[test]
    fn the_percentage_score_is_stored_on_the_0_to_100_scale() {
        // Kitsu reports "82.27" and AniList reports 84 on the same scale, so
        // the value is rounded but not multiplied by ten.
        let c = conn();
        upsert(&c, &resource("12"), &ExternalIds::default()).unwrap();
        assert_eq!(get_i64(&c, "ks:12", "score"), Some(82));
        assert_eq!(
            get_str(&c, "ks:12", "score_source").as_deref(),
            Some("kitsu")
        );
    }

    #[test]
    fn an_unparsable_or_missing_score_leaves_the_source_empty() {
        // A "score_source" with no score is how the detail page ends up
        // claiming a rating that does not exist.
        let c = conn();
        let mut r = resource("12");
        r.attributes.average_rating = Some("н/д".into());
        upsert(&c, &r, &ExternalIds::default()).unwrap();
        assert_eq!(get_i64(&c, "ks:12", "score"), None);
        assert_eq!(get_str(&c, "ks:12", "score_source"), None);

        let mut r2 = resource("13");
        r2.id = "13".into();
        r2.attributes.average_rating = None;
        upsert(&c, &r2, &ExternalIds::default()).unwrap();
        assert_eq!(get_i64(&c, "ks:13", "score"), None);
        assert_eq!(get_str(&c, "ks:13", "score_source"), None);
    }

    #[test]
    fn the_synopsis_is_stored_as_plain_text() {
        let c = conn();
        upsert(&c, &resource("12"), &ExternalIds::default()).unwrap();
        assert_eq!(
            get_str(&c, "ks:12", "description").as_deref(),
            Some("A bounty hunter crew.")
        );
    }

    #[test]
    fn the_banner_falls_back_to_the_poster_original() {
        let c = conn();
        let mut r = resource("12");
        r.attributes.cover_image = None;
        r.attributes.poster_image.original = Some("o.jpg".into());
        upsert(&c, &r, &ExternalIds::default()).unwrap();
        assert_eq!(get_str(&c, "ks:12", "banner").as_deref(), Some("o.jpg"));
    }

    #[test]
    fn the_banner_prefers_the_cover_when_there_is_one() {
        let c = conn();
        upsert(&c, &resource("12"), &ExternalIds::default()).unwrap();
        assert_eq!(get_str(&c, "ks:12", "banner").as_deref(), Some("cover.jpg"));
    }

    #[test]
    fn the_age_rating_classification_is_stored_as_json() {
        let c = conn();
        upsert(&c, &resource("12"), &ExternalIds::default()).unwrap();
        let raw = get_str(&c, "ks:12", "classifications_json").unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["ageRating"], json!("R17+"));
    }

    #[test]
    fn an_nsfw_flag_becomes_the_adult_column() {
        let c = conn();
        let mut r = resource("12");
        r.attributes.nsfw = Some(true);
        upsert(&c, &r, &ExternalIds::default()).unwrap();
        assert_eq!(get_i64(&c, "ks:12", "is_adult"), Some(1));

        let mut r2 = resource("13");
        r2.id = "13".into();
        r2.attributes.nsfw = None;
        upsert(&c, &r2, &ExternalIds::default()).unwrap();
        // Kitsu not saying is not the same as saying "not adult", but the
        // column is NOT NULL DEFAULT 0, so the row has to land as 0.
        assert_eq!(get_i64(&c, "ks:13", "is_adult"), Some(0));
    }

    #[test]
    fn every_title_locale_lands_in_alt_titles() {
        let c = conn();
        let mut r = resource("12");
        r.attributes.titles.pt_br = Some("Cowboy Bebop (PT)".into());
        upsert(&c, &r, &ExternalIds::default()).unwrap();
        let raw = get_str(&c, "ks:12", "alt_titles").unwrap();
        let v: Vec<String> = serde_json::from_str(&raw).unwrap();
        assert!(v.contains(&"カウボーイビバップ".to_string()));
        assert!(v.contains(&"Cowboy Bebop (PT)".to_string()));
    }

    #[test]
    fn a_reimport_does_not_overwrite_the_curated_anilist_title() {
        // AniList's naming is hand-curated, so a Kitsu duplicate must enrich
        // the row rather than rename it.
        let c = conn();
        upsert(
            &c,
            &resource("12"),
            &ExternalIds {
                anilist_id: Some(16498),
                ..ExternalIds::default()
            },
        )
        .unwrap();
        c.execute(
            "UPDATE anime SET title_romaji = 'Shingeki no Kyojin', score = 90, score_source = 'anilist'
             WHERE uid = 'al:16498'",
            [],
        )
        .unwrap();

        upsert(
            &c,
            &resource("12"),
            &ExternalIds {
                anilist_id: Some(16498),
                ..ExternalIds::default()
            },
        )
        .unwrap();
        assert_eq!(
            get_str(&c, "al:16498", "title_romaji").as_deref(),
            Some("Shingeki no Kyojin")
        );
        assert_eq!(get_i64(&c, "al:16498", "score"), Some(90));
        assert_eq!(
            get_str(&c, "al:16498", "score_source").as_deref(),
            Some("anilist")
        );
    }

    #[test]
    fn a_reimport_fills_in_what_kitsu_knows_and_anilist_did_not() {
        let c = conn();
        c.execute(
            "INSERT INTO anime (uid, anilist_id, title_romaji, created_at) VALUES ('al:16498', 16498, 'X', 1)",
            [],
        )
        .unwrap();
        upsert(
            &c,
            &resource("12"),
            &ExternalIds {
                anilist_id: Some(16498),
                ..ExternalIds::default()
            },
        )
        .unwrap();
        assert_eq!(get_i64(&c, "al:16498", "rating_count"), Some(500_000));
        assert_eq!(get_i64(&c, "al:16498", "favourites"), Some(20_000));
    }

    // ------------------------------------------------------------- genres

    #[test]
    fn kitsu_genres_become_rows_in_the_genres_table() {
        let c = conn();
        let v = json!({ "data": [
            { "attributes": { "name": "Action", "slug": "action", "dimension": "explicitly_requested" } },
            { "attributes": { "name": "Slice of Life", "slug": "slice of life" } },
            { "attributes": { "name": "  " } },
            { "nothing": true }
        ]});
        store_kitsu_genres(&c, &v).unwrap();

        let rows: Vec<(String, Option<String>, Option<String>)> = {
            let mut stmt = c
                .prepare("SELECT name_en, name_ru, category FROM genres ORDER BY name_en")
                .unwrap();
            let mapped = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .unwrap();
            mapped.map(|r| r.unwrap()).collect()
        };
        assert_eq!(
            rows.len(),
            2,
            "пустое имя и запись без attributes пропущены"
        );
        assert_eq!(rows[0].0, "Action");
        // `explicitly_requested` is Kitsu's word for "category", as opposed to
        // "tag" — the filter sheet shows the two groups separately.
        assert_eq!(rows[0].2.as_deref(), Some("category"));
        assert_eq!(rows[1].2.as_deref(), Some("genre"));
    }

    #[test]
    fn a_genre_without_a_slug_falls_back_to_a_normalised_key() {
        let c = conn();
        let v = json!({ "data": [
            { "attributes": { "name": "Sci-Fi" } }
        ]});
        store_kitsu_genres(&c, &v).unwrap();
        let slug: String = c
            .query_row("SELECT slug FROM genres", [], |r| r.get(0))
            .unwrap();
        assert_eq!(slug, "sci-fi");
    }

    #[test]
    fn a_genre_response_without_data_is_an_error() {
        // A wrong-shaped answer must be visible, not silently "no genres".
        let c = conn();
        assert!(store_kitsu_genres(&c, &json!({ "errors": [] })).is_err());
        assert!(store_kitsu_genres(&c, &json!({ "data": "not an array" })).is_err());
    }

    #[test]
    fn re_importing_the_genre_reference_does_not_duplicate_rows() {
        let c = conn();
        let v = json!({ "data": [{ "attributes": { "name": "Action", "slug": "action" } }]});
        store_kitsu_genres(&c, &v).unwrap();
        store_kitsu_genres(&c, &v).unwrap();
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM genres", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn the_known_russian_genre_names_are_filled_in() {
        // The UI prefers the Russian label, and this table is where it comes
        // from for the labels a user actually filters by.
        let c = conn();
        let v = json!({ "data": [
            { "attributes": { "name": "Action", "slug": "action" } },
            { "attributes": { "name": "Sci-Fi", "slug": "sci-fi" } }
        ]});
        store_kitsu_genres(&c, &v).unwrap();
        upsert(&c, &resource("12"), &ExternalIds::default()).unwrap();

        let ru: Vec<String> = {
            let mut stmt = c
                .prepare("SELECT name_ru FROM genres ORDER BY slug")
                .unwrap();
            let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };
        assert_eq!(
            ru,
            vec!["Боевик".to_string(), "Научная фантастика".to_string()]
        );
    }

    #[test]
    fn a_custom_russian_label_is_not_overwritten() {
        // Shikimori supplies better names for the same slugs; a re-import must
        // not throw them away.
        let c = conn();
        let v = json!({ "data": [{ "attributes": { "name": "Action", "slug": "action" } }]});
        store_kitsu_genres(&c, &v).unwrap();
        c.execute(
            "UPDATE genres SET name_ru = 'Боевик (Shikimori)' WHERE slug = 'action'",
            [],
        )
        .unwrap();
        upsert(&c, &resource("12"), &ExternalIds::default()).unwrap();
        let ru: String = c
            .query_row(
                "SELECT name_ru FROM genres WHERE slug = 'action'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ru, "Боевик (Shikimori)");
    }
}
