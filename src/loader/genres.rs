use super::Ctx;
use crate::error::{log_error, log_info, now_ts};
use rusqlite::{params, Connection};
use std::time::Instant;

const BATCH: usize = 2_000;

/// Walks the catalogue and links every genre-ish label to a row in `genres`.
///
/// v1 created one `genres_dict` row per distinct AniList *tag*, which meant
/// several thousand mostly-junk entries ("Male Protagonist", "Primarily Teenage
/// Cast", …) all competing with the dozen genres a user actually filters by.
/// The split below keeps the two apart: `category = 'genre'` is the small
/// curated list, `category = 'tag'` is everything else and is only exposed
/// under a separate search box.
pub async fn match_all(ctx: Ctx) -> Result<(), String> {
    let t0 = Instant::now();
    let mut total = 0i64;

    loop {
        if ctx.abort_requested() {
            log_info("[genres] прервано по запросу");
            return Ok(());
        }

        let batch: Vec<String> = {
            let c = ctx.db.conn().map_err(|e| e.to_string())?;
            let mut stmt = c
                .prepare("SELECT uid FROM anime WHERE genres_matched_at IS NULL ORDER BY rowid LIMIT ?1")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([BATCH as i64], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?;
            rows.filter_map(|r| r.ok()).collect()
        };

        if batch.is_empty() {
            break;
        }

        let db = ctx.db.clone();
        let ids = batch.clone();
        let done = tokio::task::spawn_blocking(move || {
            let conn = db.conn().map_err(|e| e.to_string())?;
            let n = match_uids(&conn, &ids);
            let _ = apply_static_ru_sync(&conn);
            Ok::<i64, String>(n as i64)
        })
        .await
        .map_err(|e| e.to_string())??;

        total += done;
        if total % 20_000 == 0 {
            log_info(&format!("[genres] сопоставлено {}", total));
        }
    }

    apply_static_ru(&ctx).await.ok();
    let pruned = prune_empty(&ctx).await.unwrap_or(0);
    let (genres, tags): (i64, i64) = {
        let db = ctx.db.clone();
        let conn = db.conn().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT COALESCE(SUM(category = 'genre'), 0), COALESCE(SUM(category = 'tag'), 0) FROM genres",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?
    };

    log_info(&format!(
        "[genres] готово за {}: жанров {}, тегов {} (удалено пустых: {})",
        super::human_secs(t0.elapsed().as_secs()),
        genres,
        tags,
        pruned
    ));
    Ok(())
}

/// Links genres for a set of rows that a source just wrote.
///
/// Genre matching used to run once, after every source had finished. A full
/// import takes hours, so the genre filter was empty for the whole of it. The
/// loaders now call this per page and the end-of-run pass is only a safety net
/// for rows a crash left unmarked.
pub fn match_uids(conn: &Connection, uids: &[String]) -> usize {
    let mut n = 0usize;
    let tx = match conn.unchecked_transaction() {
        Ok(t) => t,
        Err(e) => {
            log_error(&format!("[genres] транзакция не открыта: {}", e));
            return 0;
        }
    };
    for uid in uids {
        match link_one(&tx, uid) {
            Ok(()) => n += 1,
            Err(e) => log_error(&format!("[genres] {}: {}", uid, e)),
        }
    }
    if let Err(e) = tx.commit() {
        log_error(&format!("[genres] коммит: {}", e));
        return 0;
    }
    n
}

/// Links one row. Expects to run inside a transaction owned by the caller.
fn link_one(conn: &Connection, uid: &str) -> Result<(), rusqlite::Error> {
    let row: (Option<String>, Option<String>, Option<String>, Option<String>) = conn.query_row(
        "SELECT genres_json, tags_json, studios_json, classifications_json FROM anime WHERE uid = ?1",
        [uid],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;

    // AniList genres are the curated list.
    if let Some(s) = &row.0 {
        for name in parse_string_array(s) {
            attach(conn, uid, &name, "genre", "anilist")?;
        }
    }

    // AniList tags are descriptors, not genres.
    if let Some(s) = &row.1 {
        for name in parse_object_names(s) {
            attach(conn, uid, &name, "tag", "anilist")?;
        }
    }

    if let Some(s) = &row.2 {
        for name in parse_object_names(s) {
            attach(conn, uid, &name, "studio", "kitsu")?;
        }
    }

    conn.execute(
        "UPDATE anime SET genres_matched_at = ?1 WHERE uid = ?2",
        params![now_ts(), uid],
    )?;
    Ok(())
}

fn attach(tx: &Connection, uid: &str, name: &str, category: &str, source: &str) -> Result<(), rusqlite::Error> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Ok(());
    }
    let slug = super::title_key(name);
    if slug.is_empty() {
        return Ok(());
    }

    tx.execute(
        "INSERT INTO genres (slug, name_en, category, created_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(slug) DO UPDATE SET
            name_en  = COALESCE(genres.name_en, excluded.name_en),
            category = CASE WHEN genres.category = 'tag' AND excluded.category = 'genre'
                            THEN 'genre' ELSE genres.category END",
        params![slug, name, category, now_ts()],
    )?;
    let gid: i64 = tx.query_row("SELECT id FROM genres WHERE slug = ?1", [&slug], |r| r.get(0))?;
    tx.execute(
        "INSERT OR IGNORE INTO anime_genres (uid, genre_id, source) VALUES (?1, ?2, ?3)",
        params![uid, gid, source],
    )?;
    Ok(())
}

fn parse_string_array(json: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(json).unwrap_or_default()
}

fn parse_object_names(json: &str) -> Vec<String> {
    serde_json::from_str::<Vec<serde_json::Value>>(json)
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
        .collect()
}

/// A handful of Russian labels that cover the filters people actually use.
const STATIC_RU: &[(&str, &str)] = &[
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

async fn apply_static_ru(ctx: &Ctx) -> Result<(), String> {
    let db = ctx.db.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db.conn().map_err(|e| e.to_string())?;
        apply_static_ru_sync(&conn)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Blocking form of the Russian genre labels, so the per-page path can call it
/// without going through the async wrapper.
fn apply_static_ru_sync(conn: &Connection) -> Result<(), String> {
    for (slug, ru) in STATIC_RU {
        // `slug` is the lowercased label, so "Sci-Fi" is stored as "sci-fi".
        conn.execute(
            "UPDATE genres SET name_ru = ?1 WHERE slug = ?2 AND (name_ru IS NULL OR name_ru = '')",
            params![ru, slug],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Genres nothing links to are leftovers from an interrupted run.
async fn prune_empty(ctx: &Ctx) -> Result<usize, String> {
    let db = ctx.db.clone();
    tokio::task::spawn_blocking(move || {
        let conn = db.conn().map_err(|e| e.to_string())?;
        conn.execute(
            "DELETE FROM genres WHERE id NOT IN (SELECT genre_id FROM anime_genres)",
            [],
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Convenience for the API: a genre row with its anime count.
pub struct GenreRow {
    pub id: i64,
    pub slug: String,
    pub name_en: String,
    pub name_ru: Option<String>,
    pub category: Option<String>,
    pub count: i64,
}

pub fn list_genres(
    conn: &Connection,
    category: Option<&str>,
    min_count: i64,
) -> Result<Vec<GenreRow>, rusqlite::Error> {
    // The count is an aggregate, so the threshold belongs in HAVING. v1 wrote
    // `WHERE count > 0` against the same alias, which SQLite rejects outright
    // ("misuse of aggregate") — so the genre list never worked at all.
    let mut sql = String::from(
        "SELECT g.id, g.slug, g.name_en, g.name_ru, g.category, COUNT(ag.uid) AS c
         FROM genres g
         LEFT JOIN anime_genres ag ON ag.genre_id = g.id
         WHERE 1 = 1",
    );
    if category.is_some() {
        sql.push_str(" AND g.category = ?2");
    }
    sql.push_str(" GROUP BY g.id HAVING c > ?1 ORDER BY c DESC, g.name_en ASC");

    let mut stmt = conn.prepare(&sql)?;
    let map = |r: &rusqlite::Row<'_>| {
        Ok(GenreRow {
            id: r.get(0)?,
            slug: r.get(1)?,
            name_en: r.get(2)?,
            name_ru: r.get(3)?,
            category: r.get(4)?,
            count: r.get(5)?,
        })
    };

    let rows = match category {
        Some(c) => stmt.query_map(params![min_count, c], map)?,
        None => stmt.query_map(params![min_count], map)?,
    };
    rows.collect()
}

