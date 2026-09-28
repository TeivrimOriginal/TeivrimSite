use super::Ctx;
use crate::db;
use crate::error::{log_error, log_info, log_warn, now_ts};
use crate::sources::shikimori::{self, Anime};
use rusqlite::{params, Connection, OptionalExtension};
use std::time::Instant;

const SOURCE: &str = "shikimori";
const MAX_PAGES: u32 = 1_200;

pub async fn run(ctx: Ctx) -> Result<(), String> {
    let t0 = Instant::now();
    let per_page = 50u32;

    // Fail fast and loudly: if Shikimori is blocked, the Russian half of the
    // catalogue silently degrades to nothing, so say so instead.
    match shikimori::ping(&ctx.sources.shikimori).await {
        Ok(Some(ru)) => log_info(&format!("[shikimori] доступен, пример: «{}»", ru)),
        Ok(None) => log_warn("[shikimori] доступен, но поле russian пустое"),
        Err(e) => {
            log_error(&format!("[shikimori] недоступен ({}), русские названия не будут обновлены", e));
            return Ok(());
        }
    }

    for order in ["popularity", "rating"] {
        if ctx.abort_requested() {
            log_info("[shikimori] прервано по запросу");
            return Ok(());
        }
        let task = format!("order:{}", order);
        let cp = with_conn(&ctx, |c| Ok(db::get_checkpoint(c, SOURCE, &task)))?;
        if cp.finished {
            log_info(&format!("[shikimori][{}] уже синхронизирован, пропускаю", order));
            continue;
        }

        let mut saved = 0i64;
        let mut merged = 0i64;
        let mut page = (cp.last_page + 1).max(1) as u32;
        let mut failures = super::Failures::new();
        let mut last_reported = Instant::now();

        while page <= MAX_PAGES {
            let fetched = match shikimori::fetch_page(&ctx.sources.shikimori, page, per_page, order).await {
                Ok(f) => f,
                Err(e) => {
                    log_error(&format!("[shikimori][{}] стр. {}: {}", order, page, e));
                    let _ = with_conn(&ctx, |c| { db::mark_error(c, SOURCE, &task, &e); Ok(()) });
                    match failures.record() {
                        super::OnError::NextPage => {
                            page += 1;
                            continue;
                        }
                        super::OnError::NextSort => {
                            log_error(&format!(
                                "[shikimori][{}] {} ошибок подряд, следующий порядок",
                                order,
                                failures.streak()
                            ));
                            break;
                        }
                    }
                }
            };
            failures.reset();

            if fetched.items.is_empty() {
                with_conn(&ctx, |c| db::save_checkpoint(c, SOURCE, &task, page as i64, cp.total_saved + saved, true))?;
                break;
            }

            let mut ok = 0i64;
            let mut hit = 0i64;
            let mut touched: Vec<String> = Vec::with_capacity(fetched.items.len());
            for item in &fetched.items {
                match with_conn(&ctx, |c| upsert(c, item)) {
                    Ok(Merge::Merged) => {
                        ok += 1;
                        hit += 1;
                    }
                    Ok(Merge::Created) => {
                        ok += 1;
                        touched.push(format!("sh:{}", item.id));
                    }
                    Err(e) => log_warn(&format!("[shikimori] id={}: {}", item.id, e)),
                }
            }
            saved += ok;
            merged += hit;

            with_conn(&ctx, |c| {
                db::save_checkpoint(c, SOURCE, &task, page as i64, cp.total_saved + saved, !fetched.has_next)
            })?;

            // Shikimori carries Russian genre names, so new `sh:` rows need to
            // be linked immediately for those labels to appear.
            if !touched.is_empty() {
                let _ = with_conn(&ctx, |c| {
                    let n = super::genres::match_uids(c, &touched);
                    apply_genre_ru(c).ok();
                    Ok(n)
                });
            }

            if !fetched.has_next || page % 20 == 0 || last_reported.elapsed().as_secs() >= 15 {
                last_reported = Instant::now();
                let ru_total = with_conn(&ctx, |c| {
                    c.query_row(
                        "SELECT COUNT(*) FROM anime WHERE title_russian IS NOT NULL AND title_russian <> ''",
                        [],
                        |r| r.get::<_, i64>(0),
                    )
                })
                .unwrap_or(0);
                log_info(&format!(
                    "[shikimori][{}] стр. {}: +{} (из них {} присоединено к существующим), с русским названием: {}",
                    order, page, ok, hit, ru_total
                ));
            }

            if !fetched.has_next {
                break;
            }
            page += 1;
        }

        log_info(&format!("[shikimori][{}] присоединено {}", order, merged));
    }

    let _ = with_conn(&ctx, apply_genre_ru);
    log_info(&format!(
        "[shikimori] готово за {}",
        super::human_secs(t0.elapsed().as_secs())
    ));
    Ok(())
}

enum Merge {
    /// Attached the Russian title to a row another source already created.
    Merged,
    /// Shikimori knows a title nothing else does.
    Created,
}

fn with_conn<T>(ctx: &Ctx, f: impl FnOnce(&Connection) -> Result<T, rusqlite::Error>) -> Result<T, String> {
    let c = ctx.db.conn().map_err(|e| e.to_string())?;
    f(&c).map_err(|e| {
        log_error(&format!("db: {}", e));
        e.to_string()
    })
}

/// Shikimori is the only source with proper Russian genre names, so it is where
/// `genres.name_ru` gets filled in for the common tags.
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
    ("slice of life", "Повседневность"),
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
    ("female protagonist", "Главная героиня"),
];

fn apply_genre_ru(conn: &Connection) -> Result<(), rusqlite::Error> {
    for (en, ru) in GENRE_RU {
        conn.execute(
            "UPDATE genres SET name_ru = ?1 WHERE LOWER(name_en) = LOWER(?2) AND (name_ru IS NULL OR name_ru = '')",
            params![ru, en],
        )?;
    }
    Ok(())
}

fn upsert(conn: &Connection, item: &Anime) -> Result<Merge, rusqlite::Error> {
    let russian = item.russian.as_deref().unwrap_or("").trim();
    if russian.is_empty() {
        // Nothing to contribute: Shikimori's only unique value here is the
        // Russian name, and a row without one would be a bare stub.
        return Ok(Merge::Merged);
    }

    let name = item.name.as_deref().unwrap_or("").trim();
    let key = super::title_key(name);

    // Shikimori exposes no external ids, so the join is by title. The v1 code
    // matched with `title_romaji = ? OR title_english = ?`, which is
    // case-sensitive in SQLite for anything outside ASCII and could match an
    // unrelated title. Matching the normalised key of the romaji title first,
    // and only then the English one, is both case- and whitespace-insensitive.
    let existing: Option<String> = if key.is_empty() {
        None
    } else {
        conn.query_row(
            "SELECT uid FROM anime WHERE title_key = ?1 ORDER BY (anilist_id IS NULL) ASC LIMIT 1",
            params![key],
            |r| r.get(0),
        )
        .optional()?
    }
    .or_else(|| {
        let en = item.english.as_ref().and_then(|v| v.first()).map(|s| super::title_key(s));
        match en {
            Some(enk) if !enk.is_empty() => conn
                .query_row(
                    "SELECT uid FROM anime WHERE LOWER(COALESCE(title_english,'')) = ?1 LIMIT 1",
                    params![enk],
                    |r| r.get::<_, String>(0),
                )
                .optional()
                .ok()
                .flatten(),
            _ => None,
        }
    });

    let description_ru = item.description.as_deref().map(super::strip_html);
    let studios = item.studios.as_ref().map(|s| {
        serde_json::json!(s
            .iter()
            .map(|x| serde_json::json!({ "name": x.name, "name_ru": x.russian }))
            .collect::<Vec<_>>())
    });
    let studios_json = studios.as_ref().map(|v| v.to_string());

    // Shikimori scores 0..10; the catalogue stores 0..100.
    let score: Option<i64> = item.score.map(|s| (s * 10.0).round() as i64);

    if let Some(uid) = existing {
        conn.execute(
            "UPDATE anime SET
                title_russian       = ?1,
                description_ru      = COALESCE(?2, description_ru),
                shikimori_id        = COALESCE(shikimori_id, ?3),
                score               = COALESCE(anime.score, ?4),
                score_source        = COALESCE(anime.score_source, CASE WHEN ?4 IS NULL THEN NULL ELSE 'shikimori' END),
                episodes            = COALESCE(anime.episodes, ?5),
                start_year          = COALESCE(anime.start_year, ?6),
                start_date          = COALESCE(anime.start_date, ?7),
                cover_large         = COALESCE(anime.cover_large, ?8),
                studios_json        = COALESCE(anime.studios_json, ?9),
                updated_at          = ?10,
                shikimori_synced_at = ?10
             WHERE uid = ?11",
            params![
                russian,
                description_ru,
                item.id,
                score,
                item.episodes,
                year_of(item.aired_on.as_deref().or(item.released_on.as_deref())),
                item.aired_on.as_deref().or(item.released_on.as_deref()),
                item.image.as_ref().and_then(|i| i.original.clone()),
                studios_json,
                now_ts(),
                uid,
            ],
        )?;
        return Ok(Merge::Merged);
    }

    // Not known to AniList or Kitsu: keep it under a `sh:` uid so the id space
    // stays disjoint instead of borrowing a negative number.
    let uid = format!("sh:{}", item.id);
    conn.execute(
        r#"INSERT INTO anime (
            uid, shikimori_id,
            title_romaji, title_russian, title_key,
            format, status, description_ru, episodes,
            score, score_source,
            start_year, start_date,
            cover_large, studios_json,
            is_adult, created_at, updated_at, shikimori_synced_at
        ) VALUES (
            ?1, ?2,
            ?3, ?4, ?5,
            ?6, ?7, ?8, ?9,
            ?10, ?11,
            ?12, ?13,
            ?14, ?15,
            0, ?16, ?16, ?16
        )
        ON CONFLICT(uid) DO UPDATE SET
            title_russian  = excluded.title_russian,
            description_ru = COALESCE(excluded.description_ru, anime.description_ru),
            score          = COALESCE(excluded.score, anime.score),
            studios_json   = COALESCE(excluded.studios_json, anime.studios_json),
            updated_at     = excluded.updated_at,
            shikimori_synced_at = excluded.shikimori_synced_at
        "#,
        params![
            uid,
            item.id,
            if name.is_empty() { None } else { Some(name) },
            russian,
            if key.is_empty() { None } else { Some(&key) },
            item.kind.as_ref().map(|k| normalize_kind(k)),
            item.status.as_ref().map(|s| normalize_status(s)),
            description_ru,
            item.episodes,
            score,
            score.map(|_| "shikimori"),
            year_of(item.aired_on.as_deref().or(item.released_on.as_deref())),
            item.aired_on.as_deref().or(item.released_on.as_deref()),
            item.image.as_ref().and_then(|i| i.original.clone()),
            studios_json,
            now_ts(),
        ],
    )?;
    Ok(Merge::Created)
}

fn year_of(date: Option<&str>) -> Option<i64> {
    date.and_then(|d| d.get(0..4)).and_then(|y| y.parse().ok())
}

fn normalize_kind(s: &str) -> String {
    match s.to_ascii_lowercase().as_str() {
        "tv" => "TV".into(),
        "movie" => "MOVIE".into(),
        "ova" => "OVA".into(),
        "ona" => "ONA".into(),
        "special" => "SPECIAL".into(),
        "music" => "MUSIC".into(),
        other => other.to_string(),
    }
}

fn normalize_status(s: &str) -> String {
    match s.to_ascii_lowercase().as_str() {
        "released" | "finished" => "FINISHED".into(),
        "airing" | "current" => "RELEASING".into(),
        "not_aired" | "upcoming" => "NOT_YET_RELEASED".into(),
        "hiatus" | "on_hiatus" => "HIATUS".into(),
        "cancelled" | "discontinued" => "CANCELLED".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::testing::{conn, insert_anime};
    use crate::sources::shikimori::{ImageSet, Named, Tag};
    use serde_json::json;

    fn item(id: i64) -> Anime {
        Anime {
            id,
            name: Some("Shingeki no Kyojin".into()),
            russian: Some("Атака Титанов".into()),
            english: Some(vec!["Attack on Titan".into()]),
            japanese: Some(vec!["進撃の巨人".into()]),
            synonyms: Some(vec!["AoT".into()]),
            kind: Some("tv".into()),
            rating: Some("9.1".into()),
            score: Some(9.15),
            status: Some("released".into()),
            episodes: Some(25),
            episodes_aired: Some(25),
            aired_on: Some("2013-04-07".into()),
            released_on: None,
            description: Some("<p>Гиганты</p>".into()),
            image: Some(ImageSet {
                original: Some("https://shikimori.one/o.jpg".into()),
                preview: None,
            }),
            studios: Some(vec![Named {
                name: Some("Wit Studio".into()),
                russian: Some("Wit Studio".into()),
            }]),
            genres: Some(vec![Named { name: Some("Action".into()), russian: Some("Боевик".into()) }]),
            tags: Some(vec![Tag { name: Some("Military".into()), russian: Some("Военный".into()) }]),
            rates: None,
        }
    }

    fn get_str(c: &Connection, uid: &str, col: &str) -> Option<String> {
        c.query_row(&format!("SELECT {} FROM anime WHERE uid = ?1", col), [uid], |r| r.get(0))
            .unwrap()
    }

    fn get_i64(c: &Connection, uid: &str, col: &str) -> Option<i64> {
        c.query_row(&format!("SELECT {} FROM anime WHERE uid = ?1", col), [uid], |r| r.get(0))
            .unwrap()
    }

    // ---------------------------------------------------------- normalising

    #[test]
    fn kinds_are_folded_onto_the_shared_vocabulary() {
        // Same reason as Kitsu: one filter has to match whatever wrote the
        // column.
        for (raw, want) in [
            ("tv", "TV"),
            ("TV", "TV"),
            ("movie", "MOVIE"),
            ("ova", "OVA"),
            ("ona", "ONA"),
            ("special", "SPECIAL"),
            ("music", "MUSIC"),
        ] {
            assert_eq!(normalize_kind(raw), want, "kind {}", raw);
        }
    }

    #[test]
    fn an_unknown_kind_is_kept_verbatim() {
        assert_eq!(normalize_kind("music_video"), "music_video");
        assert_eq!(normalize_kind(""), "");
    }

    #[test]
    fn statuses_are_folded_onto_the_shared_vocabulary() {
        for (raw, want) in [
            ("released", "FINISHED"),
            ("finished", "FINISHED"),
            ("airing", "RELEASING"),
            ("current", "RELEASING"),
            ("not_aired", "NOT_YET_RELEASED"),
            ("upcoming", "NOT_YET_RELEASED"),
            ("hiatus", "HIATUS"),
            ("on_hiatus", "HIATUS"),
            ("cancelled", "CANCELLED"),
            ("discontinued", "CANCELLED"),
        ] {
            assert_eq!(normalize_status(raw), want, "status {}", raw);
        }
    }

    #[test]
    fn an_unknown_status_is_kept_verbatim() {
        assert_eq!(normalize_status("announced"), "announced");
    }

    #[test]
    fn a_year_is_read_off_the_head_of_a_date() {
        assert_eq!(year_of(Some("2013-04-07")), Some(2013));
        assert_eq!(year_of(Some("2013")), Some(2013));
        // An empty or non-numeric head must not become 0.
        assert_eq!(year_of(Some("")), None);
        assert_eq!(year_of(Some("????-04-07")), None);
        assert_eq!(year_of(Some("2013-04-07T00:00:00")), Some(2013));
        assert_eq!(year_of(None), None);
    }

    // ---------------------------------------------------------------- merge

    #[test]
    fn a_russian_name_attaches_to_an_existing_row_by_normalised_title() {
        // The join is by name because Shikimori exposes no external ids. The key
        // is what makes it case-insensitive for Cyrillic, which LIKE is not.
        let c = conn();
        insert_anime(&c, "al:16498", Some("Shingeki no Kyojin"));
        let outcome = upsert(&c, &item(16498));
        assert!(matches!(outcome, Ok(Merge::Merged)));
        assert_eq!(get_str(&c, "al:16498", "title_russian").as_deref(), Some("Атака Титанов"));
        assert_eq!(get_i64(&c, "al:16498", "shikimori_id"), Some(16498));
    }

    #[test]
    fn the_merge_is_case_insensitive() {
        // `LIKE` in SQLite folds ASCII only, which is why the stored
        // `title_key` exists; the merge has to use it.
        let c = conn();
        insert_anime(&c, "al:16498", Some("SHINGEKI NO KYOJIN"));
        assert!(matches!(upsert(&c, &item(16498)), Ok(Merge::Merged)));
        assert_eq!(get_str(&c, "al:16498", "title_russian").as_deref(), Some("Атака Титанов"));
    }

    #[test]
    fn the_merge_falls_back_to_the_english_name() {
        // A title AniList stores only under title_english still has to join.
        let c = conn();
        c.execute(
            "INSERT INTO anime (uid, title_english, created_at) VALUES ('al:1', 'Attack on Titan', 1)",
            [],
        )
        .unwrap();
        assert!(matches!(upsert(&c, &item(16498)), Ok(Merge::Merged)));
        assert_eq!(get_str(&c, "al:1", "title_russian").as_deref(), Some("Атака Титанов"));
    }

    #[test]
    fn an_unknown_title_creates_a_row_under_a_shikimori_uid() {
        let c = conn();
        assert!(matches!(upsert(&c, &item(999)), Ok(Merge::Created)));
        assert_eq!(get_str(&c, "sh:999", "title_russian").as_deref(), Some("Атака Титанов"));
        assert_eq!(get_i64(&c, "sh:999", "shikimori_id"), Some(999));
        assert_eq!(get_str(&c, "sh:999", "title_romaji").as_deref(), Some("Shingeki no Kyojin"));
        assert_eq!(get_str(&c, "sh:999", "title_key").as_deref(), Some("shingeki no kyojin"));
    }

    #[test]
    fn an_entry_without_a_russian_name_changes_nothing() {
        // The only thing Shikimori uniquely contributes is the Russian title,
        // so an entry without one is not worth a stub row.
        let c = conn();
        let mut it = item(999);
        it.russian = Some("   ".into());
        assert!(matches!(upsert(&c, &it), Ok(Merge::Merged)));
        let n: i64 = c.query_row("SELECT COUNT(*) FROM anime", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn the_ten_point_score_is_rescaled_to_the_catalogue_scale() {
        // One merged 0..100 scale across all three sources.
        let c = conn();
        upsert(&c, &item(999)).unwrap();
        assert_eq!(get_i64(&c, "sh:999", "score"), Some(92)); // 9.15 * 10 rounded
        assert_eq!(get_str(&c, "sh:999", "score_source").as_deref(), Some("shikimori"));
    }

    #[test]
    fn an_unscored_entry_stores_no_score_source() {
        let c = conn();
        let mut it = item(999);
        it.score = None;
        upsert(&c, &it).unwrap();
        assert_eq!(get_i64(&c, "sh:999", "score"), None);
        assert_eq!(get_str(&c, "sh:999", "score_source"), None);
    }

    #[test]
    fn the_merge_does_not_overwrite_a_rating_another_source_provided() {
        let c = conn();
        insert_anime(&c, "al:16498", Some("Shingeki no Kyojin"));
        c.execute("UPDATE anime SET score = 84, score_source = 'anilist' WHERE uid = 'al:16498'", [])
            .unwrap();
        upsert(&c, &item(16498)).unwrap();
        assert_eq!(get_i64(&c, "al:16498", "score"), Some(84));
        assert_eq!(get_str(&c, "al:16498", "score_source").as_deref(), Some("anilist"));
    }

    #[test]
    fn the_merge_prefers_the_russian_description_and_keeps_the_old_one_when_absent() {
        // Shikimori is the only source with a Russian synopsis, so a non-null
        // one replaces whatever was there; a row without one leaves the stored
        // text alone.
        let c = conn();
        insert_anime(&c, "al:16498", Some("Shingeki no Kyojin"));
        c.execute(
            "UPDATE anime SET description_ru = 'уже есть' WHERE uid = 'al:16498'",
            [],
        )
        .unwrap();
        upsert(&c, &item(16498)).unwrap();
        assert_eq!(get_str(&c, "al:16498", "description_ru").as_deref(), Some("Гиганты"));

        let mut bare = item(16498);
        bare.description = None;
        upsert(&c, &bare).unwrap();
        assert_eq!(get_str(&c, "al:16498", "description_ru").as_deref(), Some("Гиганты"));
    }

    #[test]
    fn the_merge_fills_in_episodes_year_and_cover() {
        let c = conn();
        insert_anime(&c, "al:16498", Some("Shingeki no Kyojin"));
        upsert(&c, &item(16498)).unwrap();
        assert_eq!(get_i64(&c, "al:16498", "episodes"), Some(25));
        assert_eq!(get_i64(&c, "al:16498", "start_year"), Some(2013));
        assert_eq!(get_str(&c, "al:16498", "start_date").as_deref(), Some("2013-04-07"));
        assert_eq!(
            get_str(&c, "al:16498", "cover_large").as_deref(),
            Some("https://shikimori.one/o.jpg")
        );
    }

    #[test]
    fn a_release_date_is_used_when_there_is_no_aired_date() {
        // Movies report `released_on` and nothing else; dropping it lost the
        // year for every film in the catalogue.
        let c = conn();
        let mut it = item(999);
        it.aired_on = None;
        it.released_on = Some("1998-04-03".into());
        upsert(&c, &it).unwrap();
        assert_eq!(get_i64(&c, "sh:999", "start_year"), Some(1998));
    }

    #[test]
    fn studios_are_stored_with_their_russian_names() {
        let c = conn();
        upsert(&c, &item(999)).unwrap();
        let raw = get_str(&c, "sh:999", "studios_json").unwrap();
        let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v[0]["name"], json!("Wit Studio"));
        assert_eq!(v[0]["name_ru"], json!("Wit Studio"));
    }

    #[test]
    fn an_entry_without_studios_stores_no_json() {
        let c = conn();
        let mut it = item(999);
        it.studios = None;
        upsert(&c, &it).unwrap();
        assert_eq!(get_str(&c, "sh:999", "studios_json"), None);
    }

    #[test]
    fn a_created_row_normalises_its_kind_and_status() {
        let c = conn();
        upsert(&c, &item(999)).unwrap();
        assert_eq!(get_str(&c, "sh:999", "format").as_deref(), Some("TV"));
        assert_eq!(get_str(&c, "sh:999", "status").as_deref(), Some("FINISHED"));
    }

    #[test]
    fn a_second_visit_does_not_duplicate_the_created_row() {
        let c = conn();
        upsert(&c, &item(999)).unwrap();
        assert!(matches!(upsert(&c, &item(999)), Ok(Merge::Merged)));
        let n: i64 = c.query_row("SELECT COUNT(*) FROM anime", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn an_entry_with_no_name_at_all_still_creates_a_keyed_row() {
        // A nameless entry must not write an empty `title_key`, which would
        // join every other nameless entry in the catalogue.
        let c = conn();
        let mut it = item(999);
        it.name = None;
        upsert(&c, &it).unwrap();
        assert_eq!(get_str(&c, "sh:999", "title_key"), None);
        assert_eq!(get_str(&c, "sh:999", "title_romaji"), None);
    }

    #[test]
    fn genres_get_their_russian_labels_from_the_name_column() {
        // Shikimori is the only source that knows "Боевик", but the genre
        // matcher only ever sees the English name, so the Russian half of the
        // filter UI comes from this table.
        let c = conn();
        insert_anime(&c, "al:1", Some("Shingeki no Kyojin"));
        c.execute(
            "UPDATE anime SET genres_json = '[\"Action\"]' WHERE uid = 'al:1'",
            [],
        )
        .unwrap();
        assert_eq!(super::super::genres::match_uids(&c, &["al:1".to_string()]), 1);
        apply_genre_ru(&c).unwrap();
        let ru: Option<String> = c
            .query_row(
                "SELECT name_ru FROM genres WHERE LOWER(name_en) = 'action'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(ru.as_deref(), Some("Боевик"));
    }

    #[test]
    fn apply_genre_ru_matches_the_name_case_insensitively() {
        // The reference list is lowercased English; the stored name is whatever
        // the source sent.
        let c = conn();
        c.execute(
            "INSERT INTO genres (slug, name_en, created_at) VALUES ('comedy', 'Comedy', 1)",
            [],
        )
        .unwrap();
        apply_genre_ru(&c).unwrap();
        let ru: Option<String> = c
            .query_row("SELECT name_ru FROM genres WHERE slug = 'comedy'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ru.as_deref(), Some("Комедия"));
    }
}
