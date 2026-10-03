use super::Ctx;
use crate::db;
use crate::error::now_ts;
use crate::error::{log_error, log_info, log_warn};
use crate::sources::anilist::{self, Media};
use rusqlite::{named_params, Connection};
use std::time::Instant;

const SOURCE: &str = "anilist";
/// Guards against a pathological loop if AniList keeps reporting next pages.
const MAX_PAGES: u32 = 4_000;

pub async fn run(ctx: Ctx) -> Result<(), String> {
    let per_page = ctx.cfg.sync_page_size;
    let t0 = Instant::now();

    for sort in anilist::SORTS {
        if ctx.abort_requested() {
            log_info("[anilist] прервано по запросу");
            return Ok(());
        }

        let task = format!("sort:{}", sort);
        let cp = with_conn(&ctx, |c| Ok(db::get_checkpoint(c, SOURCE, &task)))?;

        if cp.finished {
            log_info(&format!(
                "[anilist][{}] уже синхронизирован, пропускаю",
                sort
            ));
            continue;
        }

        let start_page = (cp.last_page + 1).max(1) as u32;
        if start_page > 1 {
            log_info(&format!(
                "[anilist][{}] продолжаю со страницы {}",
                sort, start_page
            ));
        }

        let mut saved: i64 = 0;
        let mut page = start_page;
        let mut failures = super::Failures::new();
        let mut last_reported = Instant::now();

        while page <= MAX_PAGES {
            let (media, has_next) =
                match anilist::fetch_page(&ctx.sources.anilist, page, per_page, sort).await {
                    Ok(f) => (f.media, f.has_next),
                    Err(e) => {
                        // The upstream client already retried with backoff, so a
                        // failure here means the source really is unavailable.
                        log_error(&format!("[anilist][{}] стр. {}: {}", sort, page, e));
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
                                "[anilist][{}] {} ошибок подряд, перехожу к следующей сортировке",
                                sort,
                                failures.streak()
                            ));
                                break;
                            }
                        }
                    }
                };
            failures.reset();

            if media.is_empty() {
                with_conn(&ctx, |c| {
                    db::save_checkpoint(c, SOURCE, &task, page as i64, cp.total_saved + saved, true)
                })?;
                break;
            }

            let mut ok = 0i64;
            let mut touched: Vec<String> = Vec::with_capacity(media.len());
            for m in &media {
                match with_conn(&ctx, |c| upsert(c, m)) {
                    Ok(()) => {
                        ok += 1;
                        touched.push(format!("al:{}", m.id));
                    }
                    Err(e) => log_warn(&format!("[anilist] id={}: {}", m.id, e)),
                }
            }
            saved += ok;

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

            // Link genres for this page straight away so the filter is usable
            // while the import is still running.
            if !touched.is_empty() {
                let _ = with_conn(&ctx, |c| Ok(super::genres::match_uids(c, &touched)));
            }

            // One line every 10 pages, or at most every 15 seconds, so a long
            // import does not fill the log with 1800 identical rows.
            if !has_next || page % 10 == 0 || last_reported.elapsed().as_secs() >= 15 {
                last_reported = Instant::now();
                let total = with_conn(&ctx, |c| {
                    c.query_row("SELECT COUNT(*) FROM anime", [], |r| r.get::<_, i64>(0))
                })
                .unwrap_or(0);
                log_info(&format!(
                    "[anilist][{}] стр. {}: +{} (задача: {}), в БД: {}",
                    sort, page, ok, saved, total
                ));
            }

            if !has_next {
                break;
            }
            page += 1;
        }
    }

    log_info(&format!(
        "[anilist] готово за {}",
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

fn upsert(conn: &Connection, m: &Media) -> Result<(), rusqlite::Error> {
    let t = &m.title;
    let uid = format!("al:{}", m.id);

    let (cover_xl, cover_l, cover_m, cover_color) = match &m.cover_image {
        Some(c) => (
            c.extra_large.clone(),
            c.large.clone(),
            c.medium.clone(),
            c.color.clone(),
        ),
        None => (None, None, None, None),
    };
    let (tr_id, tr_site, tr_thumb) = match &m.trailer {
        Some(t) => (t.id.clone(), t.site.clone(), t.thumbnail.clone()),
        None => (None, None, None),
    };

    let sd = m.start_date.as_ref();
    let ed = m.end_date.as_ref();
    let key = t.romaji.as_deref().map(super::title_key);

    // Any name we were not already storing goes into alt_titles so search can
    // find the row by all of them.
    let mut alt: Vec<String> = Vec::new();
    for v in [
        t.romaji.as_deref(),
        t.english.as_deref(),
        t.native.as_deref(),
        t.user_preferred.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if !v.trim().is_empty() && !alt.iter().any(|x| x == v) {
            alt.push(v.to_string());
        }
    }
    if let Some(syn) = &m.synonyms {
        for s in syn {
            if !s.trim().is_empty() && !alt.iter().any(|x| x == s) {
                alt.push(s.clone());
            }
        }
    }
    let alt_json = if alt.is_empty() {
        None
    } else {
        serde_json::to_string(&alt).ok()
    };

    let json_of =
        |v: &Option<serde_json::Value>| v.as_ref().and_then(|x| serde_json::to_string(x).ok());

    let studios = m.studios.as_ref().and_then(|s| s.nodes.as_ref()).map(|n| {
        serde_json::json!(n
            .iter()
            .map(|s| serde_json::json!({ "name": s.name, "isAnimationStudio": s.is_animation_studio }))
            .collect::<Vec<_>>())
    });

    let relations = m
        .relations
        .as_ref()
        .and_then(|r| r.edges.as_ref())
        .map(|edges| {
            serde_json::json!(edges
                .iter()
                .filter_map(|e| e.node.as_ref().map(|n| serde_json::json!({
                    "relationType": e.relation_type,
                    "id": n.id,
                    "title": n.title,
                    "format": n.format,
                    "status": n.status,
                    "cover": n.cover_image.as_ref().and_then(|c| c.large.clone()),
                })))
                .collect::<Vec<_>>())
        });

    let external_links = m.external_links.as_ref().map(|links| {
        serde_json::json!(links
            .iter()
            .map(|l| serde_json::json!({ "url": l.url, "site": l.site, "type": l.link_type }))
            .collect::<Vec<_>>())
    });

    let streaming = m.streaming_episodes.as_ref().map(|eps| {
        serde_json::json!(eps
            .iter()
            .map(|e| serde_json::json!({ "title": e.title, "thumbnail": e.thumbnail, "url": e.url, "site": e.site }))
            .collect::<Vec<_>>())
    });

    let recs = m
        .recommendations
        .as_ref()
        .and_then(|r| r.nodes.as_ref())
        .map(|nodes| {
            serde_json::json!(nodes
                .iter()
                .filter_map(
                    |n| n.media_recommendation.as_ref().map(|mm| serde_json::json!({
                        "rating": n.rating,
                        "id": mm.id,
                        "title": mm.title,
                        "format": mm.format,
                        "cover": mm.cover_image.as_ref().and_then(|c| c.large.clone()),
                    }))
                )
                .collect::<Vec<_>>())
        });

    // `is_adult` is NOT NULL in the schema and AniList sends `null` for a
    // handful of records, so an unknown flag becomes 0 rather than a failed
    // insert: before, one row with a null flag was dropped from the catalogue
    // entirely, and the loader only logged a warning about it.
    let b = |v: Option<bool>| v.map(|x| if x { 1 } else { 0 });

    // Named parameters throughout. The positional form this replaces had 52
    // columns against 49 placeholders after a column was added, and the
    // mismatch only showed up as a runtime error on every single row.
    conn.execute(
        r#"
        INSERT INTO anime (
            uid, anilist_id, mal_id,
            title_romaji, title_english, title_native, title_key, alt_titles,
            format, status, description, duration, episodes, chapters, volumes,
            country_of_origin, is_adult, is_licensed,
            season, season_year,
            start_date, end_date, start_year, start_month, start_day,
            end_year, end_month, end_day,
            score, score_source, mean_score, popularity, favourites, trending,
            cover_small, cover_medium, cover_large, cover_color, banner,
            trailer_id, trailer_site, trailer_thumbnail,
            genres_json, tags_json, studios_json, relations_json,
            external_links_json, streaming_json, recommendations_json,
            created_at, updated_at, anilist_synced_at
        ) VALUES (
            :uid, :anilist_id, :mal_id,
            :title_romaji, :title_english, :title_native, :title_key, :alt_titles,
            :format, :status, :description, :duration, :episodes, :chapters, :volumes,
            :country_of_origin, :is_adult, :is_licensed,
            :season, :season_year,
            :start_date, :end_date, :start_year, :start_month, :start_day,
            :end_year, :end_month, :end_day,
            :score, 'anilist', :mean_score, :popularity, :favourites, :trending,
            -- AniList calls these extraLarge / large / medium; the schema
            -- stores large / medium / small so a client picks by density
            -- without knowing the source vocabulary.
            :cover_small, :cover_medium, :cover_large, :cover_color, :banner,
            :trailer_id, :trailer_site, :trailer_thumbnail,
            :genres_json, :tags_json, :studios_json, :relations_json,
            :external_links_json, :streaming_json, :recommendations_json,
            :now, :now, :now
        )
        ON CONFLICT(uid) DO UPDATE SET
            anilist_id   = COALESCE(excluded.anilist_id, anime.anilist_id),
            mal_id       = COALESCE(excluded.mal_id, anime.mal_id),
            title_romaji = COALESCE(excluded.title_romaji, anime.title_romaji),
            title_english= COALESCE(excluded.title_english, anime.title_english),
            title_native = COALESCE(excluded.title_native, anime.title_native),
            title_key    = COALESCE(excluded.title_key, anime.title_key),
            alt_titles   = COALESCE(excluded.alt_titles, anime.alt_titles),
            format       = COALESCE(excluded.format, anime.format),
            status       = COALESCE(excluded.status, anime.status),
            description  = COALESCE(excluded.description, anime.description),
            duration     = COALESCE(excluded.duration, anime.duration),
            episodes     = COALESCE(excluded.episodes, anime.episodes),
            chapters     = COALESCE(excluded.chapters, anime.chapters),
            volumes      = COALESCE(excluded.volumes, anime.volumes),
            country_of_origin = COALESCE(excluded.country_of_origin, anime.country_of_origin),
            is_adult     = COALESCE(excluded.is_adult, anime.is_adult),
            is_licensed  = COALESCE(excluded.is_licensed, anime.is_licensed),
            season       = COALESCE(excluded.season, anime.season),
            season_year  = COALESCE(excluded.season_year, anime.season_year),
            start_date   = COALESCE(excluded.start_date, anime.start_date),
            end_date     = COALESCE(excluded.end_date, anime.end_date),
            start_year   = COALESCE(excluded.start_year, anime.start_year),
            start_month  = COALESCE(excluded.start_month, anime.start_month),
            start_day    = COALESCE(excluded.start_day, anime.start_day),
            end_year     = COALESCE(excluded.end_year, anime.end_year),
            end_month    = COALESCE(excluded.end_month, anime.end_month),
            end_day      = COALESCE(excluded.end_day, anime.end_day),
            score        = COALESCE(excluded.score, anime.score),
            score_source = COALESCE(excluded.score_source, anime.score_source),
            mean_score   = COALESCE(excluded.mean_score, anime.mean_score),
            popularity   = COALESCE(excluded.popularity, anime.popularity),
            favourites   = COALESCE(excluded.favourites, anime.favourites),
            trending     = COALESCE(excluded.trending, anime.trending),
            cover_small  = COALESCE(excluded.cover_small, anime.cover_small),
            cover_medium = COALESCE(excluded.cover_medium, anime.cover_medium),
            cover_large  = COALESCE(excluded.cover_large, anime.cover_large),
            cover_color  = COALESCE(excluded.cover_color, anime.cover_color),
            banner       = COALESCE(excluded.banner, anime.banner),
            trailer_id   = COALESCE(excluded.trailer_id, anime.trailer_id),
            trailer_site = COALESCE(excluded.trailer_site, anime.trailer_site),
            trailer_thumbnail = COALESCE(excluded.trailer_thumbnail, anime.trailer_thumbnail),
            genres_json  = COALESCE(excluded.genres_json, anime.genres_json),
            tags_json    = COALESCE(excluded.tags_json, anime.tags_json),
            studios_json = COALESCE(excluded.studios_json, anime.studios_json),
            relations_json = COALESCE(excluded.relations_json, anime.relations_json),
            external_links_json = COALESCE(excluded.external_links_json, anime.external_links_json),
            streaming_json = COALESCE(excluded.streaming_json, anime.streaming_json),
            recommendations_json = COALESCE(excluded.recommendations_json, anime.recommendations_json),
            updated_at   = excluded.updated_at,
            anilist_synced_at = excluded.anilist_synced_at
        "#,
        named_params! {
            ":uid": uid,
            ":anilist_id": m.id,
            ":mal_id": m.id_mal,
            ":title_romaji": t.romaji,
            ":title_english": t.english,
            ":title_native": t.native,
            ":title_key": key,
            ":alt_titles": alt_json,
            ":format": m.format,
            ":status": m.status,
            ":description": m.description.as_deref().map(super::strip_html),
            ":duration": m.duration,
            ":episodes": m.episodes,
            ":chapters": m.chapters,
            ":volumes": m.volumes,
            ":country_of_origin": m.country_of_origin,
            ":is_adult": b(m.is_adult).unwrap_or(0),
            ":is_licensed": b(m.is_licensed),
            ":season": m.season,
            ":season_year": m.season_year,
            ":start_date": sd.and_then(|d| d.to_iso()),
            ":end_date": ed.and_then(|d| d.to_iso()),
            ":start_year": sd.and_then(|d| d.year),
            ":start_month": sd.and_then(|d| d.month),
            ":start_day": sd.and_then(|d| d.day),
            ":end_year": ed.and_then(|d| d.year),
            ":end_month": ed.and_then(|d| d.month),
            ":end_day": ed.and_then(|d| d.day),
            ":score": m.average_score,
            ":mean_score": m.mean_score,
            ":popularity": m.popularity,
            ":favourites": m.favourites,
            ":trending": m.trending,
            ":cover_small": cover_m,
            ":cover_medium": cover_l,
            ":cover_large": cover_xl,
            ":cover_color": cover_color,
            ":banner": m.banner_image,
            ":trailer_id": tr_id,
            ":trailer_site": tr_site,
            ":trailer_thumbnail": tr_thumb,
            ":genres_json": m.genres.as_ref().map(|g| serde_json::to_string(g).unwrap_or_default()),
            ":tags_json": json_of(&m.tags.as_ref().map(|t| serde_json::to_value(t).unwrap_or_default())),
            ":studios_json": json_of(&studios),
            ":relations_json": json_of(&relations),
            ":external_links_json": json_of(&external_links),
            ":streaming_json": json_of(&streaming),
            ":recommendations_json": json_of(&recs),
            ":now": now_ts()
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::testing::conn;
    use crate::sources::anilist::{CoverImage, FuzzyDate, Media, Title, Trailer};

    fn media(id: i64) -> Media {
        Media {
            id,
            id_mal: Some(20),
            title: Title {
                romaji: Some("Shingeki no Kyojin".into()),
                english: Some("Attack on Titan".into()),
                native: Some("進撃の巨人".into()),
                user_preferred: Some("Shingeki no Kyojin".into()),
            },
            format: Some("TV".into()),
            status: Some("FINISHED".into()),
            description: Some("<p>Huge humanoids</p><p>Eat people</p>".into()),
            duration: Some(24),
            episodes: Some(25),
            country_of_origin: Some("JP".into()),
            is_adult: Some(false),
            is_licensed: Some(true),
            start_date: Some(FuzzyDate {
                year: Some(2013),
                month: Some(4),
                day: Some(7),
            }),
            end_date: Some(FuzzyDate {
                year: Some(2023),
                month: Some(11),
                day: Some(4),
            }),
            season: Some("SPRING".into()),
            season_year: Some(2013),
            average_score: Some(84),
            mean_score: Some(85),
            popularity: Some(12345),
            favourites: Some(6789),
            trending: Some(42),
            cover_image: Some(CoverImage {
                extra_large: Some("xl.jpg".into()),
                large: Some("l.jpg".into()),
                medium: Some("m.jpg".into()),
                color: Some("#8f9494".into()),
            }),
            banner_image: Some("banner.jpg".into()),
            trailer: Some(Trailer {
                id: Some("abc".into()),
                site: Some("youtube".into()),
                thumbnail: Some("thumb.jpg".into()),
            }),
            genres: Some(vec!["Action".into(), "Drama".into()]),
            ..Media::default()
        }
    }

    fn get_str(c: &Connection, col: &str) -> Option<String> {
        c.query_row(
            &format!("SELECT {} FROM anime WHERE uid = 'al:16498'", col),
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn get_i64(c: &Connection, col: &str) -> Option<i64> {
        c.query_row(
            &format!("SELECT {} FROM anime WHERE uid = 'al:16498'", col),
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    // ------------------------------------------------------------- mapping

    #[test]
    fn stores_a_row_under_the_anilist_uid() {
        let c = conn();
        upsert(&c, &media(16498)).unwrap();
        let n: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM anime WHERE uid = 'al:16498'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(get_i64(&c, "anilist_id"), Some(16498));
        assert_eq!(get_i64(&c, "mal_id"), Some(20));
    }

    #[test]
    fn maps_the_score_fields_and_their_source() {
        // One merged 0..100 scale: AniList's number is used verbatim and the
        // source is recorded so the UI can say where a rating came from.
        let c = conn();
        upsert(&c, &media(16498)).unwrap();
        assert_eq!(get_i64(&c, "score"), Some(84));
        assert_eq!(get_i64(&c, "mean_score"), Some(85));
        assert_eq!(get_str(&c, "score_source").as_deref(), Some("anilist"));
    }

    #[test]
    fn remaps_the_cover_sizes_to_small_medium_large() {
        // AniList names them extraLarge / large / medium; the schema names them
        // by density so a client can pick a size without knowing the source.
        let c = conn();
        upsert(&c, &media(16498)).unwrap();
        assert_eq!(get_str(&c, "cover_small").as_deref(), Some("m.jpg"));
        assert_eq!(get_str(&c, "cover_medium").as_deref(), Some("l.jpg"));
        assert_eq!(get_str(&c, "cover_large").as_deref(), Some("xl.jpg"));
        assert_eq!(get_str(&c, "cover_color").as_deref(), Some("#8f9494"));
        assert_eq!(get_str(&c, "banner").as_deref(), Some("banner.jpg"));
    }

    #[test]
    fn a_missing_cover_leaves_every_size_null() {
        // No cover must stay NULL, not become an empty string: the API layer
        // treats '' and NULL differently when it builds the placeholder URL.
        let c = conn();
        let mut m = media(16498);
        m.cover_image = None;
        upsert(&c, &m).unwrap();
        assert_eq!(get_str(&c, "cover_small"), None);
        assert_eq!(get_str(&c, "cover_large"), None);
        assert_eq!(get_str(&c, "cover_color"), None);
    }

    #[test]
    fn splits_the_fuzzy_start_date_into_a_date_and_three_numbers() {
        let c = conn();
        upsert(&c, &media(16498)).unwrap();
        assert_eq!(get_str(&c, "start_date").as_deref(), Some("2013-04-07"));
        assert_eq!(get_i64(&c, "start_year"), Some(2013));
        assert_eq!(get_i64(&c, "start_month"), Some(4));
        assert_eq!(get_i64(&c, "start_day"), Some(7));
        assert_eq!(get_str(&c, "end_date").as_deref(), Some("2023-11-04"));
        assert_eq!(get_i64(&c, "end_year"), Some(2023));
    }

    #[test]
    fn stores_a_partial_date_as_its_longest_known_prefix() {
        let c = conn();
        let mut m = media(16498);
        m.start_date = Some(FuzzyDate {
            year: Some(2024),
            month: Some(7),
            day: None,
        });
        upsert(&c, &m).unwrap();
        assert_eq!(get_str(&c, "start_date").as_deref(), Some("2024-07"));
        assert_eq!(get_i64(&c, "start_month"), Some(7));
        assert_eq!(get_i64(&c, "start_day"), None);
    }

    #[test]
    fn an_unknown_date_leaves_every_date_column_null() {
        let c = conn();
        let mut m = media(16498);
        m.start_date = None;
        m.end_date = None;
        upsert(&c, &m).unwrap();
        assert_eq!(get_str(&c, "start_date"), None);
        assert_eq!(get_i64(&c, "start_year"), None);
        assert_eq!(get_i64(&c, "end_year"), None);
    }

    #[test]
    fn stores_the_description_as_plain_text() {
        // The API serves text, never HTML: the markup must not reach a client.
        let c = conn();
        upsert(&c, &media(16498)).unwrap();
        assert_eq!(
            get_str(&c, "description").as_deref(),
            Some("Huge humanoids\nEat people")
        );
    }

    #[test]
    fn collects_every_title_variant_into_alt_titles() {
        // Search has to find a row by all of its names, including the ones the
        // grid never shows.
        let c = conn();
        let mut m = media(16498);
        m.synonyms = Some(vec!["AoT".into(), "Shingeki no Kyojin".into()]);
        upsert(&c, &m).unwrap();

        let raw = get_str(&c, "alt_titles").unwrap();
        let v: Vec<String> = serde_json::from_str(&raw).unwrap();
        // romaji, english, native and userPreferred, then the synonyms that are
        // not already in the list.
        assert!(v.contains(&"Shingeki no Kyojin".to_string()));
        assert!(v.contains(&"Attack on Titan".to_string()));
        assert!(v.contains(&"進撃の巨人".to_string()));
        assert!(v.contains(&"AoT".to_string()));
        assert_eq!(
            v.iter().filter(|x| *x == "Shingeki no Kyojin").count(),
            1,
            "userPreferred повторяет romaji и должен быть отброшен"
        );
        assert_eq!(
            get_str(&c, "title_key").as_deref(),
            Some("shingeki no kyojin")
        );
    }

    #[test]
    fn no_title_variants_means_no_alt_titles_column() {
        let c = conn();
        let mut m = media(16498);
        m.title = Title::default();
        upsert(&c, &m).unwrap();
        assert_eq!(get_str(&c, "alt_titles"), None);
        assert_eq!(get_str(&c, "title_key"), None);
        // The row itself still lands: an untitled entry is better than a lost
        // page of the import.
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM anime", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn blank_title_variants_are_not_stored_as_search_keys() {
        // Whitespace-only names would otherwise become a searchable key that
        // matches every LIKE.
        let c = conn();
        let mut m = media(16498);
        m.title = Title {
            romaji: Some("   ".into()),
            english: Some("Attack on Titan".into()),
            native: None,
            user_preferred: None,
        };
        upsert(&c, &m).unwrap();
        let raw = get_str(&c, "alt_titles").unwrap();
        let v: Vec<String> = serde_json::from_str(&raw).unwrap();
        assert_eq!(v, vec!["Attack on Titan".to_string()]);
    }

    #[test]
    fn stores_genres_and_tags_in_their_own_columns() {
        let c = conn();
        let mut m = media(16498);
        m.tags = Some(vec![crate::sources::anilist::Tag {
            name: Some("Military".into()),
            rank: Some(80),
            is_media_spoiler: Some(false),
        }]);
        upsert(&c, &m).unwrap();
        let genres: Vec<String> =
            serde_json::from_str(&get_str(&c, "genres_json").unwrap()).unwrap();
        assert_eq!(genres, vec!["Action".to_string(), "Drama".to_string()]);

        let tags: Vec<serde_json::Value> =
            serde_json::from_str(&get_str(&c, "tags_json").unwrap()).unwrap();
        // Tags are stored as objects, not strings: the detail page shows the
        // rank and the spoiler flag next to each one.
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0]["name"], serde_json::json!("Military"));
        assert_eq!(tags[0]["rank"], serde_json::json!(80));
    }

    #[test]
    fn a_row_without_genres_stores_null_not_an_empty_array() {
        // NULL and `[]` mean different things to the genre matcher, which
        // decides whether a row still needs linking.
        let c = conn();
        let mut m = media(16498);
        m.genres = None;
        m.tags = None;
        upsert(&c, &m).unwrap();
        assert_eq!(get_str(&c, "genres_json"), None);
        assert_eq!(get_str(&c, "tags_json"), None);
    }

    #[test]
    fn stores_the_trailer_parts() {
        let c = conn();
        upsert(&c, &media(16498)).unwrap();
        assert_eq!(get_str(&c, "trailer_id").as_deref(), Some("abc"));
        assert_eq!(get_str(&c, "trailer_site").as_deref(), Some("youtube"));
        assert_eq!(
            get_str(&c, "trailer_thumbnail").as_deref(),
            Some("thumb.jpg")
        );
    }

    #[test]
    fn a_row_without_a_trailer_stores_nulls() {
        let c = conn();
        let mut m = media(16498);
        m.trailer = None;
        upsert(&c, &m).unwrap();
        assert_eq!(get_str(&c, "trailer_id"), None);
        assert_eq!(get_str(&c, "trailer_site"), None);
    }

    #[test]
    fn stores_boolean_flags_as_integers() {
        let c = conn();
        let mut m = media(16498);
        m.is_adult = Some(true);
        upsert(&c, &m).unwrap();
        assert_eq!(get_i64(&c, "is_adult"), Some(1));
        assert_eq!(get_i64(&c, "is_licensed"), Some(1));

        let mut m2 = media(16499);
        m2.is_adult = Some(false);
        upsert(&c, &m2).unwrap();
        let adult: i64 = c
            .query_row(
                "SELECT is_adult FROM anime WHERE uid = 'al:16499'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(adult, 0);
    }

    #[test]
    fn an_unknown_adult_flag_is_stored_as_false() {
        // The column is NOT NULL and AniList does send `null`, so the loader
        // has to decide: 0 keeps the row in the catalogue, NULL would have
        // dropped it.
        let c = conn();
        let mut m = media(16498);
        m.is_adult = None;
        upsert(&c, &m).unwrap();
        assert_eq!(get_i64(&c, "is_adult"), Some(0));
    }

    #[test]
    fn stores_the_nested_lists_as_json_arrays() {
        use crate::sources::anilist::{
            Connection as AniConnection, ExternalLink, RecommendationConnection,
            RecommendationNode, RelationConnection, RelationEdge, RelationNode, StreamingEpisode,
            Studio,
        };

        let c = conn();
        let mut m = media(16498);
        m.studios = Some(AniConnection {
            nodes: Some(vec![Studio {
                name: Some("Wit Studio".into()),
                is_animation_studio: Some(true),
            }]),
        });
        m.relations = Some(RelationConnection {
            edges: Some(vec![RelationEdge {
                relation_type: Some("PREQUEL".into()),
                node: Some(RelationNode {
                    id: 11061,
                    title: Some(Title {
                        romaji: Some("Kaban".into()),
                        ..Title::default()
                    }),
                    format: Some("MOVIE".into()),
                    status: Some("FINISHED".into()),
                    cover_image: Some(CoverImage {
                        large: Some("kaban.jpg".into()),
                        ..CoverImage::default()
                    }),
                }),
            }]),
        });
        m.external_links = Some(vec![ExternalLink {
            id: 1,
            url: Some("https://myanimelist.net/anime/20".into()),
            site: Some("MAL".into()),
            link_type: Some("ANILIST_SITE".into()),
        }]);
        m.streaming_episodes = Some(vec![StreamingEpisode {
            title: Some("Episode 1".into()),
            thumbnail: None,
            url: Some("https://anilist.co/watch/1".into()),
            site: Some("anilist".into()),
        }]);
        m.recommendations = Some(RecommendationConnection {
            nodes: Some(vec![RecommendationNode {
                rating: Some(95),
                media_recommendation: Some(crate::sources::anilist::RecommendedMedia {
                    id: 127230,
                    title: Some(Title {
                        romaji: Some("Gingitsune".into()),
                        ..Title::default()
                    }),
                    format: Some("TV_SHORT".into()),
                    cover_image: None,
                }),
            }]),
        });
        upsert(&c, &m).unwrap();

        for col in [
            "studios_json",
            "relations_json",
            "external_links_json",
            "streaming_json",
            "recommendations_json",
        ] {
            let raw = get_str(&c, col).unwrap_or_else(|| panic!("{} пуст", col));
            let v: serde_json::Value =
                serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{} не JSON: {}", col, e));
            assert!(v.is_array(), "{} не массив", col);
            assert_eq!(v.as_array().unwrap().len(), 1, "{} пуст", col);
        }

        let relations: serde_json::Value =
            serde_json::from_str(&get_str(&c, "relations_json").unwrap()).unwrap();
        assert_eq!(relations[0]["relationType"], serde_json::json!("PREQUEL"));
        assert_eq!(relations[0]["id"], serde_json::json!(11061));
    }

    #[test]
    fn a_relation_with_a_null_node_is_dropped() {
        use crate::sources::anilist::{RelationConnection, RelationEdge};
        let c = conn();
        let mut m = media(16498);
        m.relations = Some(RelationConnection {
            edges: Some(vec![
                RelationEdge {
                    relation_type: Some("PREQUEL".into()),
                    node: None,
                },
                RelationEdge {
                    relation_type: Some("SEQUEL".into()),
                    node: None,
                },
            ]),
        });
        upsert(&c, &m).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&get_str(&c, "relations_json").unwrap()).unwrap();
        assert!(v.as_array().unwrap().is_empty());
    }

    #[test]
    fn empty_connection_lists_are_stored_as_empty_arrays() {
        use crate::sources::anilist::{Connection as AniConnection, RelationConnection};
        let c = conn();
        let mut m = media(16498);
        m.studios = Some(AniConnection {
            nodes: Some(vec![]),
        });
        m.relations = Some(RelationConnection {
            edges: Some(vec![]),
        });
        upsert(&c, &m).unwrap();
        assert_eq!(get_str(&c, "studios_json").as_deref(), Some("[]"));
        assert_eq!(get_str(&c, "relations_json").as_deref(), Some("[]"));
    }

    // -------------------------------------------------------------- re-import

    #[test]
    fn a_second_import_does_not_erase_what_it_no_longer_knows() {
        // COALESCE on the update path: a title that lost its English name on
        // the source must keep the one we already have, otherwise the grid
        // flickers to "Без названия" on every re-sync.
        let c = conn();
        upsert(&c, &media(16498)).unwrap();

        let mut thin = media(16498);
        thin.title = Title {
            romaji: Some("Shingeki no Kyojin".into()),
            ..Title::default()
        };
        thin.cover_image = None;
        thin.trailer = None;
        thin.average_score = None;
        thin.episodes = None;
        thin.description = None;
        upsert(&c, &thin).unwrap();

        assert_eq!(
            get_str(&c, "title_english").as_deref(),
            Some("Attack on Titan")
        );
        assert_eq!(get_str(&c, "cover_large").as_deref(), Some("xl.jpg"));
        assert_eq!(get_i64(&c, "score"), Some(84));
        assert_eq!(get_i64(&c, "episodes"), Some(25));
        assert!(get_str(&c, "description").is_some());
        assert_eq!(
            get_str(&c, "title_romaji").as_deref(),
            Some("Shingeki no Kyojin")
        );

        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM anime", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "повторный импорт не должен плодить строки");
    }

    #[test]
    fn a_second_import_refreshes_the_sync_timestamp() {
        let c = conn();
        upsert(&c, &media(16498)).unwrap();
        let first: i64 = get_i64(&c, "updated_at").unwrap();
        upsert(&c, &media(16498)).unwrap();
        let second: i64 = get_i64(&c, "updated_at").unwrap();
        assert!(second >= first);
        assert!(get_i64(&c, "anilist_synced_at").is_some());
    }
}
