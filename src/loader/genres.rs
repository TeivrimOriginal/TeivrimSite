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
    //
    // The comparison is inclusive: the parameter is named `min_count` and the
    // default is 1, so `>` silently dropped every genre attached to exactly
    // one title — which is most of the long tail in the `tag` category.
    let mut sql = String::from(
        "SELECT g.id, g.slug, g.name_en, g.name_ru, g.category, COUNT(ag.uid) AS c
         FROM genres g
         LEFT JOIN anime_genres ag ON ag.genre_id = g.id
         WHERE 1 = 1",
    );
    if category.is_some() {
        sql.push_str(" AND g.category = ?2");
    }
    sql.push_str(" GROUP BY g.id HAVING c >= ?1 ORDER BY c DESC, g.name_en ASC");

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::testing::conn;
    use rusqlite::Connection;
    use serde_json::json;

    /// One catalogue row with the given JSON blobs, as a source loader would
    /// have left it.
    fn row(conn: &Connection, uid: &str, genres: Option<&str>, tags: Option<&str>, studios: Option<&str>) {
        conn.execute(
            "INSERT INTO anime (uid, genres_json, tags_json, studios_json, created_at)
             VALUES (?1, ?2, ?3, ?4, 1)",
            params![uid, genres, tags, studios],
        )
        .unwrap();
    }

    fn slugs(conn: &Connection) -> Vec<String> {
        let mut stmt = conn.prepare("SELECT slug FROM genres ORDER BY slug").unwrap();
        let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    // ------------------------------------------------------------- linking

    #[test]
    fn anilist_genres_become_genre_rows() {
        let c = conn();
        row(&c, "al:1", Some(r#"["Action","Slice of Life"]"#), None, None);
        assert_eq!(match_uids(&c, &["al:1".to_string()]), 1);
        assert_eq!(slugs(&c), vec!["action".to_string(), "slice of life".to_string()]);

        let category: String = c
            .query_row("SELECT category FROM genres WHERE slug = 'action'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(category, "genre");
    }

    #[test]
    fn anilist_tags_become_tag_rows_and_not_genres() {
        // This split is the whole point of the rewrite: v1 created one row per
        // distinct tag, so "Male Protagonist" sat next to "Action" in the same
        // filter list.
        let c = conn();
        row(
            &c,
            "al:1",
            Some(r#"["Action"]"#),
            Some(r#"[{"name":"Male Protagonist","rank":90,"isMediaSpoiler":false}]"#),
            None,
        );
        match_uids(&c, &["al:1".to_string()]);

        let category: String = c
            .query_row("SELECT category FROM genres WHERE slug = 'male protagonist'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(category, "tag");
    }

    #[test]
    fn kitsu_studios_become_studio_rows() {
        let c = conn();
        row(
            &c,
            "ks:1",
            None,
            None,
            Some(r#"[{"name":"Wit Studio","isAnimationStudio":true}]"#),
        );
        match_uids(&c, &["ks:1".to_string()]);
        let category: String = c
            .query_row("SELECT category FROM genres WHERE slug = 'wit studio'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(category, "studio");
    }

    #[test]
    fn a_label_linked_as_a_tag_is_promoted_when_a_genre_claims_it() {
        // AniList tags and genres overlap; whichever is more specific wins, and
        // it must not flip back on the next pass.
        let c = conn();
        row(
            &c,
            "al:1",
            Some(r#"["Military"]"#),
            Some(r#"[{"name":"Military"}]"#),
            None,
        );
        match_uids(&c, &["al:1".to_string()]);
        let category: String = c
            .query_row("SELECT category FROM genres WHERE slug = 'military'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(category, "genre");
    }

    #[test]
    fn a_broken_json_blob_does_not_stop_the_row() {
        // A truncated blob from a crashed import must not lose the whole row:
        // the link step is what makes the genre filter usable.
        let c = conn();
        row(&c, "al:1", Some(r#"["Action", "#), Some("{oops"), Some("nope"));
        assert_eq!(match_uids(&c, &["al:1".to_string()]), 1);
        assert!(slugs(&c).is_empty(), "битые данные не должны порождать строки");

        let marked: Option<i64> = c
            .query_row("SELECT genres_matched_at FROM anime WHERE uid = 'al:1'", [], |r| r.get(0))
            .unwrap();
        assert!(marked.is_some(), "строка всё равно помечена обработанной");
    }

    #[test]
    fn a_null_blob_is_simply_nothing_to_link() {
        let c = conn();
        row(&c, "al:1", None, None, None);
        assert_eq!(match_uids(&c, &["al:1".to_string()]), 1);
        assert!(slugs(&c).is_empty());
    }

    #[test]
    fn empty_and_overlong_labels_are_ignored() {
        // An empty slug would match every other row; an 80+ character label is
        // a whole sentence, not a genre.
        let c = conn();
        let long = "x".repeat(81);
        row(
            &c,
            "al:1",
            Some(&json!(["", "   ", long, "Action"]).to_string()),
            None,
            None,
        );
        match_uids(&c, &["al:1".to_string()]);
        assert_eq!(slugs(&c), vec!["action".to_string()]);
    }

    #[test]
    fn linking_the_same_row_twice_is_idempotent() {
        // The loader calls this per page and again from the final pass.
        let c = conn();
        row(&c, "al:1", Some(r#"["Action"]"#), None, None);
        match_uids(&c, &["al:1".to_string()]);
        match_uids(&c, &["al:1".to_string()]);
        assert_eq!(slugs(&c), vec!["action".to_string()]);
        let links: i64 = c
            .query_row("SELECT COUNT(*) FROM anime_genres", [], |r| r.get(0))
            .unwrap();
        assert_eq!(links, 1);
    }

    #[test]
    fn an_unknown_uid_is_reported_and_the_batch_continues() {
        let c = conn();
        row(&c, "al:1", Some(r#"["Action"]"#), None, None);
        assert_eq!(
            match_uids(&c, &["al:missing".to_string(), "al:1".to_string()]),
            1,
            "пропущенная строка не должна прерывать пачку"
        );
    }

    #[test]
    fn an_empty_batch_is_a_no_op() {
        let c = conn();
        assert_eq!(match_uids(&c, &[]), 0);
    }

    #[test]
    fn two_rows_sharing_a_genre_get_one_genre_row_and_two_links() {
        let c = conn();
        row(&c, "al:1", Some(r#"["Action"]"#), None, None);
        row(&c, "al:2", Some(r#"["action"]"#), None, None);
        match_uids(&c, &["al:1".to_string(), "al:2".to_string()]);
        assert_eq!(slugs(&c), vec!["action".to_string()]);
        let links: i64 = c.query_row("SELECT COUNT(*) FROM anime_genres", [], |r| r.get(0)).unwrap();
        assert_eq!(links, 2);
    }

    // ------------------------------------------------------------- listing

    #[test]
    fn list_genres_counts_links_and_keeps_even_the_rare_ones() {
        let c = conn();
        row(&c, "al:1", Some(r#"["Action","Drama"]"#), None, None);
        row(&c, "al:2", Some(r#"["Action"]"#), None, None);
        row(&c, "al:3", Some(r#"["Horror"]"#), None, None);
        match_uids(&c, &["al:1".to_string(), "al:2".to_string(), "al:3".to_string()]);
        apply_static_ru_sync(&c).unwrap();

        // `min_count` is inclusive, so a genre attached to a single title is
        // still offered — most of the long tail looks like that.
        let rows = list_genres(&c, None, 1).unwrap();
        let names: Vec<&str> = rows.iter().map(|r| r.name_en.as_str()).collect();
        assert_eq!(names, vec!["Action", "Drama", "Horror"]);
        assert_eq!(rows[0].count, 2);
        assert_eq!(rows[2].count, 1);
        // The static table is the source of the Russian labels the filter sheet
        // shows.
        assert_eq!(rows[0].name_ru.as_deref(), Some("Боевик"));
    }

    #[test]
    fn list_genres_honours_a_higher_threshold() {
        let c = conn();
        row(&c, "al:1", Some(r#"["Action","Drama"]"#), None, None);
        row(&c, "al:2", Some(r#"["Action"]"#), None, None);
        match_uids(&c, &["al:1".to_string(), "al:2".to_string()]);
        let rows = list_genres(&c, None, 2).unwrap();
        let names: Vec<&str> = rows.iter().map(|r| r.name_en.as_str()).collect();
        assert_eq!(names, vec!["Action"]);
    }

    #[test]
    fn list_genres_orders_by_count_then_name() {
        let c = conn();
        row(&c, "al:1", Some(r#"["Action"]"#), None, None);
        row(&c, "al:2", Some(r#"["Drama"]"#), None, None);
        row(&c, "al:3", Some(r#"["Action","Drama"]"#), None, None);
        match_uids(&c, &["al:1".to_string(), "al:2".to_string(), "al:3".to_string()]);
        let rows = list_genres(&c, None, 1).unwrap();
        let names: Vec<&str> = rows.iter().map(|r| r.name_en.as_str()).collect();
        assert_eq!(names, vec!["Action", "Drama"], "две ссылки идут раньше одной");
        assert_eq!(rows[0].count, 2);
        assert_eq!(rows[1].count, 2);
    }

    #[test]
    fn list_genres_can_be_restricted_to_one_category() {
        let c = conn();
        row(
            &c,
            "al:1",
            Some(r#"["Action"]"#),
            Some(r#"[{"name":"Military"}]"#),
            None,
        );
        match_uids(&c, &["al:1".to_string()]);
        assert_eq!(list_genres(&c, Some("genre"), 1).unwrap().len(), 1);
        assert_eq!(list_genres(&c, Some("tag"), 1).unwrap().len(), 1);
        assert_eq!(list_genres(&c, Some("studio"), 1).unwrap().len(), 0);
    }

    #[test]
    fn list_genres_of_an_empty_catalogue_is_empty() {
        let c = conn();
        assert!(list_genres(&c, None, 1).unwrap().is_empty());
    }

    #[test]
    fn list_genres_accepts_a_zero_threshold() {
        let c = conn();
        row(&c, "al:1", Some(r#"["Action"]"#), None, None);
        // Unlinked genres are pruned at the end of a sync; until then the list
        // endpoint has to tolerate a minimum of 0, which means "including
        // genres nothing links to yet".
        c.execute(
            "INSERT INTO genres (slug, name_en, category, created_at) VALUES ('orphan', 'Orphan', 'genre', 1)",
            [],
        )
        .unwrap();
        let rows = list_genres(&c, None, 0).unwrap();
        assert!(rows.iter().any(|r| r.name_en == "Orphan" && r.count == 0));
    }

    // ----------------------------------------------------------- json input

    #[test]
    fn parse_string_array_is_total() {
        assert!(parse_string_array("not json").is_empty());
        assert!(parse_string_array("{}").is_empty());
        assert_eq!(parse_string_array(r#"["a","b"]"#), vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn parse_object_names_takes_only_the_name_field() {
        assert_eq!(
            parse_object_names(r#"[{"name":"A"},{"name":"B","rank":1},{"rank":2}]"#),
            vec!["A".to_string(), "B".to_string()]
        );
        assert!(parse_object_names("[1,2,3]").is_empty());
    }
}

