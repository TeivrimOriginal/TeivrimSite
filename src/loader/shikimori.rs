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
        let mut errors = 0u32;
        let mut last_reported = Instant::now();

        while page <= MAX_PAGES {
            let fetched = match shikimori::fetch_page(&ctx.sources.shikimori, page, per_page, order).await {
                Ok(f) => f,
                Err(e) => {
                    errors += 1;
                    log_error(&format!("[shikimori][{}] стр. {}: {}", order, page, e));
                    let _ = with_conn(&ctx, |c| { db::mark_error(c, SOURCE, &task, &e); Ok(()) });
                    if errors >= 3 {
                        log_error(&format!("[shikimori][{}] три ошибки подряд, следующий порядок", order));
                        break;
                    }
                    page += 1;
                    continue;
                }
            };
            errors = 0;

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
