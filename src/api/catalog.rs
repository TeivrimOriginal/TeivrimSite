use crate::error::ApiResult;
use crate::models::*;
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, Row};
use std::collections::BTreeMap;

/// A WHERE clause under construction, with positional parameters collected in
/// the same order the `?` markers appear.
pub struct Where {
    parts: Vec<String>,
    params: Vec<SqlValue>,
}

impl Where {
    pub fn new() -> Where {
        Where {
            parts: Vec::new(),
            params: Vec::new(),
        }
    }

    fn push(&mut self, sql: &str, value: SqlValue) {
        self.parts.push(sql.to_string());
        self.params.push(value);
    }


    fn raw(&mut self, sql: &str) {
        self.parts.push(sql.to_string());
    }


    pub fn sql(&self) -> String {
        if self.parts.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", self.parts.join(" AND "))
        }
    }

    pub fn values(&self) -> &[SqlValue] {
        &self.params
    }
}

/// Sort orders accepted from clients. Anything else falls back to the default,
/// which is the important part: the value is interpolated straight into SQL, so
/// it must never come from the request unvalidated.
pub fn order_clause(sort: &str) -> &'static str {
    match sort {
        "score" => "ORDER BY score DESC NULLS LAST, popularity DESC NULLS LAST, rating_count DESC",
        "score_asc" => "ORDER BY score ASC NULLS LAST, rating_count ASC",
        "rating_count" => "ORDER BY rating_count DESC NULLS LAST",
        "rating_count_asc" => "ORDER BY rating_count ASC NULLS LAST",
        "favourites" => "ORDER BY favourites DESC NULLS LAST",
        "trending" => "ORDER BY trending DESC NULLS LAST",
        "year" => "ORDER BY start_year DESC NULLS LAST, start_month DESC NULLS LAST, start_day DESC NULLS LAST",
        "year_asc" => "ORDER BY start_year ASC NULLS LAST",
        "title" => "ORDER BY COALESCE(NULLIF(title_romaji,''), title_english) COLLATE NOCASE ASC",
        "title_desc" => "ORDER BY COALESCE(NULLIF(title_romaji,''), title_english) COLLATE NOCASE DESC",
        "title_ru" => "ORDER BY COALESCE(NULLIF(title_russian,''), NULLIF(title_romaji,''), title_english) COLLATE NOCASE ASC",
        "episodes" => "ORDER BY episodes DESC NULLS LAST",
        "episodes_asc" => "ORDER BY episodes ASC NULLS LAST",
        "duration" => "ORDER BY duration DESC NULLS LAST",
        "added" => "ORDER BY created_at DESC NULLS LAST",
        "updated" => "ORDER BY updated_at DESC NULLS LAST",
        "id" => "ORDER BY anilist_id IS NULL, anilist_id ASC",
        "id_desc" => "ORDER BY anilist_id IS NULL, anilist_id DESC",
        _ => "ORDER BY popularity DESC NULLS LAST, anilist_id ASC NULLS LAST",
    }
}

/// Uppercases a controlled-vocabulary value and maps the spellings people
/// actually type onto what the sources store.
///
/// `to_uppercase` and not `to_ascii_uppercase`: the table carries Russian
/// synonyms («тв», «завершён»), and folding ASCII only meant that typing them
/// in lower case produced a filter that matched nothing.
fn norm_enum(raw: &str, kind: &str) -> Option<String> {
    let v = raw.trim().to_uppercase();
    if v.is_empty() {
        return None;
    }
    let mapped = match (kind, v.as_str()) {
        ("format", "TV" | "TV_SHORT" | "SERIES" | "ТВ") => "TV",
        ("format", "MOVIE" | "FILM" | "FILMЫ" | "ФИЛЬМ") => "MOVIE",
        ("format", "OVA") => "OVA",
        ("format", "ONA") => "ONA",
        ("format", "SPECIAL" | "SPECIALS") => "SPECIAL",
        ("format", "MUSIC" | "CLIP") => "MUSIC",
        ("status", "FINISHED" | "RELEASED" | "COMPLETED" | "ЗАВЕРШЁН" | "ЗАВЕРШЕН") => "FINISHED",
        ("status", "RELEASING" | "AIRING" | "CURRENT" | "ВЫХОДИТ" | "ИДЁТ") => "RELEASING",
        ("status", "NOT_YET_RELEASED" | "UPCOMING" | "NOT_RELEASED" | "НЕ ВЫШЕЛ" | "ПРЕДСТОИТ") => {
            "NOT_YET_RELEASED"
        }
        ("status", "HIATUS" | "ON_HIATUS" | "ПРИОСТАНОВЛЕН") => "HIATUS",
        ("status", "CANCELLED" | "CANCELED" | "ОТМЕНЁН" | "ОТМЕНЕН") => "CANCELLED",
        (_, _) => return Some(v),
    };
    Some(mapped.to_string())
}

/// Builds the WHERE clause for a list request.
///
/// Fixes carried over from v1, where this function was the source of two user
/// visible bugs:
///   * the year range used `(start_year IS NULL OR start_year >= ?)`, so every
///     title with an unknown year was returned no matter which range you asked
///     for — the "1990s" filter silently included yearless entries;
///   * the genre filter matched with `genres_json LIKE '%"Action"%'`, which
///     cannot use an index, breaks on any name containing a quote, and ignored
///     the `anime_genres` table entirely.
pub fn build_where(
    q: &ListQuery,
    fts: bool,
    user_id: Option<i64>,
    genres_by_id: &BTreeMap<i64, Genre>,
) -> Where {
    let mut w = Where::new();

    // ---- free text ----
    if let Some(raw) = q.q.as_ref() {
        let term = raw.trim();
        if !term.is_empty() {
            if fts {
                if let Some(expr) = fts_expr(term) {
                    // uid is UNINDEXED in the FTS table, so the join key comes
                    // back without a second lookup.
                    w.raw(&format!(
                        "a.uid IN (SELECT uid FROM anime_fts WHERE anime_fts MATCH '{}')",
                        expr
                    ));
                }
            } else {
                // Degraded path, used only when SQLite was built without FTS5.
                // LIKE folds case for ASCII only, so Cyrillic search is weaker
                // here; that limitation is logged once at startup.
                let like = format!("%{}%", escape_like(term));
                w.parts.push(
                    "a.uid IN (SELECT uid FROM anime \
                       WHERE title_romaji LIKE ? ESCAPE '\\' \
                          OR title_english LIKE ? ESCAPE '\\' \
                          OR title_native LIKE ? ESCAPE '\\' \
                          OR title_russian LIKE ? ESCAPE '\\' \
                          OR alt_titles LIKE ? ESCAPE '\\')"
                        .into(),
                );
                for _ in 0..5 {
                    w.params.push(SqlValue::Text(like.clone()));
                }
            }
        }
    }

    // ---- controlled vocabularies ----
    if let Some(f) = q.format.as_deref().and_then(|v| norm_enum(v, "format")) {
        w.push("a.format = ?", SqlValue::Text(f));
    }
    if let Some(s) = q.status.as_deref().and_then(|v| norm_enum(v, "status")) {
        w.push("a.status = ?", SqlValue::Text(s));
    }
    if let Some(s) = q.season.as_deref().and_then(|v| norm_enum(v, "season")) {
        w.push("a.season = ?", SqlValue::Text(s));
    }
    if let Some(c) = q.country.as_deref() {
        let c = c.trim().to_ascii_uppercase();
        if !c.is_empty() {
            w.push("a.country_of_origin = ?", SqlValue::Text(c));
        }
    }
    if let Some(y) = q.season_year {
        w.push("a.season_year = ?", SqlValue::Text(y.to_string()));
    }

    // ---- numeric ranges ----
    // NULL years are excluded on purpose: an entry with no known year is not in
    // any decade, and including it made every range filter look broken.
    if let Some(y) = q.year_from {
        w.push("a.start_year IS NOT NULL AND a.start_year >= ?", SqlValue::Integer(y));
    }
    if let Some(y) = q.year_to {
        w.push("a.start_year IS NOT NULL AND a.start_year <= ?", SqlValue::Integer(y));
    }
    if let Some(v) = q.score_from {
        w.push("a.score IS NOT NULL AND a.score >= ?", SqlValue::Integer(v));
    }
    if let Some(v) = q.score_to {
        w.push("a.score IS NOT NULL AND a.score <= ?", SqlValue::Integer(v));
    }
    if let Some(v) = q.duration_from {
        w.push("a.duration >= ?", SqlValue::Integer(v));
    }
    if let Some(v) = q.duration_to {
        w.push("a.duration <= ?", SqlValue::Integer(v));
    }
    if let Some(v) = q.episodes_from {
        w.push("a.episodes >= ?", SqlValue::Integer(v));
    }
    if let Some(v) = q.episodes_to {
        w.push("a.episodes <= ?", SqlValue::Integer(v));
    }

    // `year=2024` shorthand, and `year=2010-2015`.
    if q.year_from.is_none() && q.year_to.is_none() {
        if let Some(y) = q.year.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            if let Some((a, b)) = y.split_once('-') {
                if let (Ok(a), Ok(b)) = (a.trim().parse::<i64>(), b.trim().parse::<i64>()) {
                    w.push("a.start_year >= ?", SqlValue::Integer(a));
                    w.push("a.start_year <= ?", SqlValue::Integer(b));
                }
            } else if let Ok(v) = y.parse::<i64>() {
                w.push("a.start_year = ?", SqlValue::Integer(v));
            }
        }
    }

    // ---- flags ----
    match q.adult.as_deref() {
        Some("only") => w.raw("a.is_adult = 1"),
        Some("no") | Some("safe") => w.raw("a.is_adult = 0"),
        _ => {}
    }
    match q.licensed.as_deref() {
        Some("yes") => w.raw("a.is_licensed = 1"),
        Some("no") => w.raw("a.is_licensed = 0"),
        _ => {}
    }
    if matches!(q.has_russian.as_deref(), Some("yes" | "1" | "true")) {
        w.raw("(a.title_russian IS NOT NULL AND a.title_russian <> '')");
    }
    if matches!(q.has_trailer.as_deref(), Some("yes" | "1" | "true")) {
        w.raw("(a.trailer_id IS NOT NULL AND a.trailer_id <> '')");
    }

    // ---- taxonomy joins ----
    for (param, category) in [(q.genre.as_deref(), "genre"), (q.tag.as_deref(), "tag"), (q.studio.as_deref(), "studio")] {
        let Some(slugs) = param.map(slug_list) else { continue };
        if slugs.is_empty() {
            continue;
        }
        let ph = vec!["?"; slugs.len()].join(",");
        w.parts.push(format!(
            "a.uid IN (SELECT ag.uid FROM anime_genres ag \
             JOIN genres g ON g.id = ag.genre_id \
             WHERE g.slug IN ({ph}) AND (g.category = ? OR g.category IS NULL))"
        ));
        for s in &slugs {
            w.params.push(SqlValue::Text(s.clone()));
        }
        w.params.push(SqlValue::Text(category.to_string()));
    }

    // ---- watchlist, needs a signed-in user ----
    if let (Some(uid_user), Some(list)) = (user_id, q.in_list.as_deref()) {
        match list {
            "favorites" | "favourites" => {
                w.raw(
                    "a.uid IN (SELECT uid FROM favorites WHERE user_id = ? AND is_favorite = 1)",
                );
                w.params.push(SqlValue::Integer(uid_user));
            }
            "all" => {
                w.raw("a.uid IN (SELECT uid FROM favorites WHERE user_id = ?)");
                w.params.push(SqlValue::Integer(uid_user));
            }
            other if VALID_STATUSES.contains(&other) => {
                w.parts
                    .push("a.uid IN (SELECT uid FROM favorites WHERE user_id = ? AND status = ?)".into());
                w.params.push(SqlValue::Integer(uid_user));
                w.params.push(SqlValue::Text(other.to_string()));
            }
            _ => {}
        }
    }

    let _ = genres_by_id;
    w
}

pub const VALID_STATUSES: &[&str] = &["watching", "planned", "completed", "dropped", "paused"];

fn slug_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(crate::loader::title_key)
        .filter(|s| !s.is_empty())
        .collect()
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// Turns user input into an FTS5 MATCH expression: every whitespace-separated
/// token becomes a quoted prefix term, all required. `unicode61` folds case
/// across scripts, which is what makes `АТАКА` match `Атака` — the thing plain
/// `LIKE` could never do.
pub fn fts_expr(input: &str) -> Option<String> {
    let mut terms: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in input.chars() {
        if ch.is_alphanumeric() || ch == '_' {
            cur.push(ch);
        } else {
            if !cur.is_empty() {
                terms.push(std::mem::take(&mut cur));
            }
            if ch == '"' {
                // A quoted phrase: keep the inner words as one prefix term.
            }
        }
    }
    if !cur.is_empty() {
        terms.push(cur);
    }
    if terms.is_empty() {
        return None;
    }
    Some(
        terms
            .iter()
            .map(|t| format!("\"{}\"*", t.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" AND "),
    )
}

// ==================================================================
// Row mapping
// ==================================================================

const SUMMARY_COLUMNS: &str = "a.uid, \
    a.title_romaji, a.title_english, a.title_russian, a.title_native, \
    a.cover_large, a.cover_medium, a.cover_small, a.cover_color, \
    a.score, a.score_source, a.format, a.status, a.episodes, a.duration, \
    a.start_year, a.season, a.season_year, a.country_of_origin, a.is_adult, \
    a.genres_json";

/// Exposed so the watchlist query can join onto the same projection and reuse
/// the same row mapper.
pub fn summary_columns() -> &'static str {
    SUMMARY_COLUMNS
}

pub fn summary_from_row(row: &Row<'_>) -> rusqlite::Result<AnimeSummary> {
    let title_romaji: Option<String> = row.get(1)?;
    let title_english: Option<String> = row.get(2)?;
    let title_russian: Option<String> = row.get(3)?;
    let title_native: Option<String> = row.get(4)?;
    let cover_large: Option<String> = row.get(5)?;
    let cover_medium: Option<String> = row.get(6)?;
    let cover_small: Option<String> = row.get(7)?;
    let genres_json: Option<String> = row.get(20)?;

    let title = title_russian
        .clone()
        .or_else(|| title_romaji.clone())
        .or_else(|| title_english.clone())
        .or_else(|| title_native.clone())
        .unwrap_or_else(|| "Без названия".to_string());

    Ok(AnimeSummary {
        uid: row.get(0)?,
        title,
        title_romaji,
        title_english,
        title_russian,
        title_native,
        cover: cover_large.or(cover_medium).or(cover_small),
        cover_color: row.get(8)?,
        score: row.get(9)?,
        score_source: row.get(10)?,
        format: row.get(11)?,
        status: row.get(12)?,
        episodes: row.get(13)?,
        duration: row.get(14)?,
        year: row.get(15)?,
        season: row.get(16)?,
        season_year: row.get(17)?,
        country: row.get(18)?,
        is_adult: row.get::<_, Option<i64>>(19)?.unwrap_or(0) == 1,
        genres: parse_genres(&genres_json),
    })
}

pub fn parse_genres(json: &Option<String>) -> Vec<Genre> {
    let Some(s) = json else { return Vec::new() };
    let arr: Vec<String> = serde_json::from_str(s).unwrap_or_default();
    arr.into_iter()
        .map(|name| Genre {
            id: 0,
            slug: crate::loader::title_key(&name),
            name,
            name_ru: None,
            category: Some("genre".into()),
            count: None,
        })
        .collect()
}

/// Runs the paged list query. Blocking; call inside `web::block`.
pub fn query_list(
    conn: &Connection,
    q: &ListQuery,
    fts: bool,
    user_id: Option<i64>,
    max_per_page: i64,
) -> ApiResult<Paged<AnimeSummary>> {
    let per_page = q.per_page.unwrap_or(48).clamp(1, max_per_page);
    let page = q.page.unwrap_or(1).max(1);
    let offset = (page as i64 - 1) * per_page;
    let order = order_clause(q.sort.as_deref().unwrap_or("popularity"));
    let where_ = build_where(q, fts, user_id, &BTreeMap::new());

    let total: i64 = {
        let sql = format!("SELECT COUNT(*) FROM anime a{}", where_.sql());
        conn.query_row(&sql, rusqlite::params_from_iter(where_.values()), |r| r.get(0))?
    };

    let sql = format!(
        "SELECT {} FROM anime a{} {} LIMIT ? OFFSET ?",
        SUMMARY_COLUMNS,
        where_.sql(),
        order
    );
    let mut values: Vec<SqlValue> = where_.values().to_vec();
    values.push(SqlValue::Integer(per_page));
    values.push(SqlValue::Integer(offset));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(values.iter()), summary_from_row)?;
    let mut items = Vec::with_capacity(per_page as usize);
    for r in rows {
        match r {
            Ok(item) => items.push(item),
            // One malformed row must not blank the whole page.
            Err(e) => crate::error::log_warn(&format!("[api] строка пропущена: {}", e)),
        }
    }

    let total_pages = if per_page > 0 { (total + per_page - 1) / per_page } else { 0 };
    let has_more = items.len() as i64 == per_page && offset + per_page < total;
    Ok(Paged {
        items,
        page,
        per_page,
        total,
        total_pages,
        has_more,
    })
}

/// Type-ahead suggestions, resolved through the same index as the main search
/// but without the taxonomy joins, so it stays a single cheap query.
pub fn query_suggest(conn: &Connection, term: &str, fts: bool, limit: i64) -> ApiResult<Vec<Suggestion>> {
    let limit = limit.clamp(1, 20);
    let term = term.trim();
    if term.is_empty() {
        return Ok(Vec::new());
    }

    let (sql, values): (String, Vec<SqlValue>) = if fts {
        match fts_expr(term) {
            Some(expr) => (
                // `rank` is FTS5's bm25 score, lower being better; ordering by
                // it makes exact prefix hits float to the top.
                format!(
                    "SELECT a.uid, \
                            COALESCE(NULLIF(a.title_russian,''), NULLIF(a.title_romaji,''), \
                                     NULLIF(a.title_english,''), NULLIF(a.title_native,'')), \
                            a.title_romaji, a.title_english, a.title_russian, a.title_native, \
                            COALESCE(a.cover_large, a.cover_medium, a.cover_small), a.popularity, a.score \
                     FROM anime_fts f JOIN anime a ON a.uid = f.uid \
                     WHERE anime_fts MATCH '{}' \
                     ORDER BY bm25(anime_fts), a.popularity DESC LIMIT ?",
                    expr
                ),
                vec![SqlValue::Integer(limit)],
            ),
            None => return Ok(Vec::new()),
        }
    } else {
        let like = format!("%{}%", escape_like(term));
        (
            "SELECT a.uid, \
                    COALESCE(NULLIF(a.title_russian,''), NULLIF(a.title_romaji,''), \
                             NULLIF(a.title_english,''), NULLIF(a.title_native,'')), \
                    a.title_romaji, a.title_english, a.title_russian, a.title_native, \
                    COALESCE(a.cover_large, a.cover_medium, a.cover_small), a.popularity, a.score \
             FROM anime a \
             WHERE a.title_romaji LIKE ? ESCAPE '\\' OR a.title_english LIKE ? ESCAPE '\\' \
                OR a.title_russian LIKE ? ESCAPE '\\' OR a.title_native LIKE ? ESCAPE '\\' \
             ORDER BY a.popularity DESC LIMIT ?"
                .to_string(),
            vec![
                SqlValue::Text(like.clone()),
                SqlValue::Text(like.clone()),
                SqlValue::Text(like.clone()),
                SqlValue::Text(like),
                SqlValue::Integer(limit),
            ],
        )
    };

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(values.iter()), |row| {
        Ok(Suggestion {
            uid: row.get(0)?,
            title: row.get(1)?,
            title_romaji: row.get(2)?,
            title_english: row.get(3)?,
            title_russian: row.get(4)?,
            title_native: row.get(5)?,
            cover: row.get(6)?,
            popularity: row.get(7)?,
            score: row.get(8)?,
        })
    })?;

    let mut out = Vec::new();
    for r in rows.flatten() {
        out.push(r);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::testing::{conn, insert_anime};
    use rusqlite::params;
    use serde_json::json;

    fn q(json: serde_json::Value) -> ListQuery {
        serde_json::from_value(json).unwrap()
    }

    fn no_genres() -> BTreeMap<i64, Genre> {
        BTreeMap::new()
    }

    // ------------------------------------------------------------- sorting

    #[test]
    fn every_documented_sort_maps_to_a_clause() {
        for s in [
            "score", "score_asc", "rating_count", "rating_count_asc", "favourites", "trending",
            "year", "year_asc", "title", "title_desc", "title_ru", "episodes", "episodes_asc",
            "duration", "added", "updated", "id", "id_desc",
        ] {
            let clause = order_clause(s);
            assert!(clause.starts_with("ORDER BY "), "sort {} -> {}", s, clause);
        }
    }

    #[test]
    fn an_unknown_sort_falls_back_to_popularity() {
        // The value is interpolated straight into SQL, so anything that is not
        // on the whitelist must be dropped, not escaped.
        for hostile in ["id; DROP TABLE anime", "", "1", "score--", "ORDER BY 1"] {
            assert_eq!(order_clause(hostile), order_clause("popularity"), "sort {:?}", hostile);
        }
    }

    #[test]
    fn nulls_sort_last_in_a_descending_score() {
        // An unscored title must land at the end of the "top" list, not first.
        assert!(order_clause("score").contains("score DESC NULLS LAST"));
    }

    #[test]
    fn source_only_titles_sort_after_numbered_ones() {
        // `sh:` rows have no anilist_id, and putting them first would fill the
        // first page with long-tail stubs.
        assert_eq!(order_clause("id"), "ORDER BY anilist_id IS NULL, anilist_id ASC");
    }

    // ------------------------------------------------------- vocabularies

    #[test]
    fn formats_are_normalised_to_the_shared_vocabulary() {
        for (raw, want) in [
            ("tv", "TV"),
            ("TV_SHORT", "TV"),
            ("series", "TV"),
            ("тв", "TV"),
            ("movie", "MOVIE"),
            ("film", "MOVIE"),
            ("фильм", "MOVIE"),
            ("specials", "SPECIAL"),
            ("OVA", "OVA"),
        ] {
            assert_eq!(norm_enum(raw, "format").as_deref(), Some(want), "format {}", raw);
        }
    }

    #[test]
    fn statuses_are_normalised_to_the_shared_vocabulary() {
        for (raw, want) in [
            ("finished", "FINISHED"),
            ("released", "FINISHED"),
            ("завершён", "FINISHED"),
            ("airing", "RELEASING"),
            ("current", "RELEASING"),
            ("идёт", "RELEASING"),
            ("upcoming", "NOT_YET_RELEASED"),
            ("hiatus", "HIATUS"),
            ("cancelled", "CANCELLED"),
            ("отменен", "CANCELLED"),
        ] {
            assert_eq!(norm_enum(raw, "status").as_deref(), Some(want), "status {}", raw);
        }
    }

    #[test]
    fn an_empty_value_is_no_filter_at_all() {
        // `?format=` in a hand-written URL must not become a filter that
        // matches nothing.
        assert_eq!(norm_enum("", "format"), None);
        assert_eq!(norm_enum("   ", "status"), None);
    }

    #[test]
    fn an_unrecognised_vocabulary_value_is_passed_through_uppercased() {
        // Seasons are not on the list, and a source may invent a new one; the
        // value still has to reach the query or the filter silently drops it.
        assert_eq!(norm_enum("winter", "season").as_deref(), Some("WINTER"));
        assert_eq!(norm_enum("made_up_format", "format").as_deref(), Some("MADE_UP_FORMAT"));
    }

    // --------------------------------------------------------- where clause

    #[test]
    fn an_empty_query_produces_no_where_clause() {
        let w = build_where(&q(json!({})), true, None, &no_genres());
        assert_eq!(w.sql(), "");
        assert!(w.values().is_empty());
    }

    #[test]
    fn a_blank_search_term_is_ignored() {
        // `?q=%20` must not become `LIKE '%%'` and return the whole catalogue.
        let w = build_where(&q(json!({ "q": "   " })), false, None, &no_genres());
        assert_eq!(w.sql(), "");
    }

    #[test]
    fn the_fts_path_uses_a_matched_subquery() {
        let w = build_where(&q(json!({ "q": "titans" })), true, None, &no_genres());
        let sql = w.sql();
        assert!(sql.contains("anime_fts MATCH"));
        assert!(sql.contains("a.uid IN (SELECT uid FROM anime_fts"));
    }

    #[test]
    fn the_like_path_escapes_wildcards() {
        // Without ESCAPE, a search for "100%" matches every title and the
        // endpoint looks like it ignores the query.
        let w = build_where(&q(json!({ "q": "100%_" })), false, None, &no_genres());
        let sql = w.sql();
        assert!(sql.contains("ESCAPE '\\'"));
        assert_eq!(w.values().len(), 5, "по одному параметру на каждое из пяти полей");
        match &w.values()[0] {
            SqlValue::Text(s) => assert_eq!(s, "%100\\%\\_%"),
            other => panic!("параметр не строка: {:?}", other),
        }
    }

    #[test]
    fn a_year_range_excludes_titles_with_no_year() {
        // v1 wrote `(start_year IS NULL OR start_year >= ?)`, so the "1990s"
        // filter included every yearless title in the catalogue.
        let w = build_where(&q(json!({ "year_from": 1990, "year_to": 1999 })), true, None, &no_genres());
        let sql = w.sql();
        assert!(sql.contains("a.start_year IS NOT NULL AND a.start_year >= ?"));
        assert!(sql.contains("a.start_year IS NOT NULL AND a.start_year <= ?"));
        assert!(!sql.contains("OR a.start_year IS NULL"));
    }

    #[test]
    fn a_score_range_excludes_unscored_titles() {
        let w = build_where(&q(json!({ "score_from": 50, "score_to": 90 })), true, None, &no_genres());
        let sql = w.sql();
        assert!(sql.contains("a.score IS NOT NULL AND a.score >= ?"));
        assert!(sql.contains("a.score IS NOT NULL AND a.score <= ?"));
    }

    #[test]
    fn the_year_shorthand_expands_to_a_range() {
        let w = build_where(&q(json!({ "year": "2010-2015" })), true, None, &no_genres());
        let sql = w.sql();
        assert!(sql.contains("a.start_year >= ?"));
        assert!(sql.contains("a.start_year <= ?"));
        assert_eq!(w.values().len(), 2);
    }

    #[test]
    fn a_single_year_shorthand_is_an_equality() {
        let w = build_where(&q(json!({ "year": "2013" })), true, None, &no_genres());
        assert!(w.sql().contains("a.start_year = ?"));
        assert_eq!(w.values().len(), 1);
    }

    #[test]
    fn explicit_year_bounds_win_over_the_shorthand() {
        // Both in one URL is a client bug; the explicit bounds are the ones the
        // caller asked for last, and mixing them produced a range nobody typed.
        let w = build_where(
            &q(json!({ "year": "2013", "year_from": 2000, "year_to": 2005 })),
            true,
            None,
            &no_genres(),
        );
        assert!(!w.sql().contains("a.start_year = ?"));
        assert_eq!(w.values().len(), 2);
    }

    #[test]
    fn a_nonsense_year_shorthand_is_ignored() {
        let w = build_where(&q(json!({ "year": "1990s" })), true, None, &no_genres());
        assert_eq!(w.sql(), "");
    }

    #[test]
    fn genre_slugs_are_normalised_before_they_reach_sql() {
        // The filter sheet sends "Sci-Fi, Action"; the table stores the
        // normalised slug, so the two have to be reconciled here.
        let w = build_where(&q(json!({ "genre": "  Sci-Fi ,Action " })), true, None, &no_genres());
        let sql = w.sql();
        assert!(sql.contains("g.slug IN (?,?)"));
        assert!(sql.contains("g.category = ?"));
        let values: Vec<String> = w
            .values()
            .iter()
            .map(|v| match v {
                SqlValue::Text(s) => s.clone(),
                other => panic!("параметр не строка: {:?}", other),
            })
            .collect();
        assert_eq!(values, vec!["sci-fi".to_string(), "action".to_string(), "genre".to_string()]);
    }

    #[test]
    fn tags_and_studios_filter_through_their_own_category() {
        for (param, want) in [("tag", "tag"), ("studio", "studio")] {
            let w = build_where(&q(json!({ param: "military" })), true, None, &no_genres());
            let last = w.values().last().unwrap();
            assert_eq!(last, &SqlValue::Text(want.to_string()));
        }
    }

    #[test]
    fn an_empty_genre_list_is_ignored() {
        // `?genre=,,,` must not become `IN (NULL)` and hide everything.
        let w = build_where(&q(json!({ "genre": " , , " })), true, None, &no_genres());
        assert_eq!(w.sql(), "");
    }

    #[test]
    fn flags_are_parsed_not_compared_as_strings() {
        assert!(build_where(&q(json!({ "adult": "only" })), true, None, &no_genres())
            .sql()
            .contains("a.is_adult = 1"));
        assert!(build_where(&q(json!({ "adult": "no" })), true, None, &no_genres())
            .sql()
            .contains("a.is_adult = 0"));
        // An unrecognised flag is no filter: the old `?adult=true` had to keep
        // working, and anything else must not exclude the adult titles.
        assert_eq!(build_where(&q(json!({ "adult": "maybe" })), true, None, &no_genres()).sql(), "");
        assert!(build_where(&q(json!({ "has_russian": "yes" })), true, None, &no_genres())
            .sql()
            .contains("a.title_russian IS NOT NULL"));
        assert!(build_where(&q(json!({ "has_trailer": "1" })), true, None, &no_genres())
            .sql()
            .contains("a.trailer_id IS NOT NULL"));
    }

    #[test]
    fn the_watchlist_filter_needs_a_signed_in_user() {
        // Anonymous: silently ignoring the parameter is right, otherwise the
        // catalogue would come back empty for anyone not logged in.
        let w = build_where(&q(json!({ "in_list": "favorites" })), true, None, &no_genres());
        assert_eq!(w.sql(), "");

        let w = build_where(&q(json!({ "in_list": "favorites" })), true, Some(7), &no_genres());
        assert!(w.sql().contains("favorites"));
        assert_eq!(w.values(), &[SqlValue::Integer(7)]);
    }

    #[test]
    fn a_named_watchlist_bucket_is_validated_against_the_known_statuses() {
        // The status is a bound parameter, so it is safe — but an unknown
        // bucket must not be honoured or the UI would show an empty tab.
        let w = build_where(&q(json!({ "in_list": "watching" })), true, Some(7), &no_genres());
        assert!(w.sql().contains("AND status = ?"));
        assert_eq!(w.values().len(), 2);

        let w = build_where(&q(json!({ "in_list": "nonsense" })), true, Some(7), &no_genres());
        assert_eq!(w.sql(), "");
    }

    #[test]
    fn several_filters_are_joined_with_and() {
        let w = build_where(
            &q(json!({ "format": "TV", "status": "FINISHED", "episodes_from": 12 })),
            true,
            None,
            &no_genres(),
        );
        let sql = w.sql();
        assert!(sql.starts_with(" WHERE "));
        assert_eq!(sql.matches(" AND ").count(), 2);
        assert_eq!(w.values().len(), 3);
    }

    // ------------------------------------------------------------ fts_expr

    #[test]
    fn fts_expr_turns_every_word_into_a_required_prefix_term() {
        // Prefix matching is what makes type-ahead useful: "titan" must find
        // "Titans" without the client knowing the full title.
        assert_eq!(fts_expr("titans").as_deref(), Some("\"titans\"*"));
        assert_eq!(
            fts_expr("attack on titan").as_deref(),
            Some("\"attack\"* AND \"on\"* AND \"titan\"*")
        );
    }

    #[test]
    fn fts_expr_drops_punctuation_but_keeps_digits_and_underscores() {
        assert_eq!(fts_expr("re:zero").as_deref(), Some("\"re\"* AND \"zero\"*"));
        assert_eq!(fts_expr("5").as_deref(), Some("\"5\"*"));
        assert_eq!(fts_expr("_x_").as_deref(), Some("\"_x_\"*"));
    }

    #[test]
    fn fts_expr_keeps_cyrillic() {
        // `unicode61` folds Cyrillic case, which plain LIKE cannot do. This is
        // the single reason the FTS path exists.
        assert_eq!(fts_expr("Атака Титанов").as_deref(), Some("\"Атака\"* AND \"Титанов\"*"));
    }

    #[test]
    fn fts_expr_of_punctuation_only_is_none() {
        // None means "do not run the query at all": a MATCH against an empty
        // expression is a SQL error.
        assert_eq!(fts_expr(""), None);
        assert_eq!(fts_expr("   "), None);
        assert_eq!(fts_expr("!!! ???"), None);
    }

    #[test]
    fn fts_expr_cannot_be_broken_out_of() {
        // The expression is interpolated into SQL, so every term has to be
        // fully quoted: a bare quote or a bare keyword must not survive.
        for hostile in ["\" OR 1=1 --", "a\"* AND anime_fts MATCH \"b", "^x", "'; DROP TABLE anime; --"] {
            let e = fts_expr(hostile).unwrap();
            for term in e.split(" AND ") {
                assert!(
                    term.starts_with('"') && term.ends_with("\"*"),
                    "не экранированный терм {:?} в выражении {:?}",
                    term,
                    e
                );
            }
        }
    }

    #[test]
    fn escape_like_neutralises_wildcards() {
        assert_eq!(escape_like("100%"), "100\\%");
        assert_eq!(escape_like("a_b"), "a\\_b");
        assert_eq!(escape_like("c:\\path"), "c:\\\\path");
        assert_eq!(escape_like("plain"), "plain");
    }

    // ------------------------------------------------------------- genres

    #[test]
    fn parse_genres_turns_the_stored_array_into_wire_objects() {
        let v = parse_genres(&Some(r#"["Action","Slice of Life"]"#.to_string()));
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "Action");
        assert_eq!(v[0].slug, "action");
        assert_eq!(v[1].slug, "slice of life");
        assert_eq!(v[0].category.as_deref(), Some("genre"));
    }

    #[test]
    fn a_missing_or_broken_genre_blob_yields_an_empty_list() {
        // The grid renders whatever is there, so a broken blob must not take
        // the whole page down.
        assert!(parse_genres(&None).is_empty());
        assert!(parse_genres(&Some("not json".into())).is_empty());
        assert!(parse_genres(&Some("{}".into())).is_empty());
    }

    // ------------------------------------------------------ running against a db

    fn seed(conn: &rusqlite::Connection) {
        let rows = [
            ("al:1", "Атака Титанов", 2013, 84, 500i64, 0i64),
            ("al:2", "Shingeki no Kyojin", 2013, 90, 900, 0),
            ("al:3", "Kimetsu no Yaiba", 2019, 78, 300, 0),
            ("ks:4", "Стальной алхимик", 2003, 82, 100, 1),
        ];
        for (uid, title, year, score, popularity, adult) in rows {
            conn.execute(
                "INSERT INTO anime (uid, title_romaji, title_key, start_year, score, popularity, is_adult, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1)",
                params![uid, title, title.to_lowercase(), year, score, popularity, adult],
            )
            .unwrap();
        }
    }

    #[test]
    fn query_list_paginates_and_counts() {
        let c = conn();
        seed(&c);
        let page = query_list(&c, &q(json!({ "per_page": 3 })), false, None, 48).unwrap();
        assert_eq!(page.items.len(), 3);
        assert_eq!(page.total, 4);
        assert_eq!(page.total_pages, 2);
        assert!(page.has_more);
        assert_eq!(page.page, 1);
        assert_eq!(page.per_page, 3);
    }

    #[test]
    fn the_last_page_reports_no_more() {
        let c = conn();
        seed(&c);
        let page = query_list(&c, &q(json!({ "per_page": 3, "page": 2 })), false, None, 48).unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(!page.has_more);
        assert_eq!(page.total_pages, 2);
    }

    #[test]
    fn the_page_size_is_capped_by_the_deployment_limit() {
        // The cap is the only thing standing between a client and a 22k row
        // response.
        let c = conn();
        seed(&c);
        let page = query_list(&c, &q(json!({ "per_page": 1000 })), false, None, 2).unwrap();
        assert_eq!(page.per_page, 2);
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.total_pages, 2);
    }

    #[test]
    fn a_zero_or_negative_page_number_is_clamped_to_one() {
        let c = conn();
        seed(&c);
        let page = query_list(&c, &q(json!({ "page": 0, "per_page": 2 })), false, None, 48).unwrap();
        assert_eq!(page.page, 1);
        assert_eq!(page.items.len(), 2);
    }

    #[test]
    fn an_empty_catalogue_returns_an_empty_but_well_formed_page() {
        let c = conn();
        let page = query_list(&c, &q(json!({})), false, None, 48).unwrap();
        assert!(page.items.is_empty());
        assert_eq!(page.total, 0);
        assert_eq!(page.total_pages, 0);
        assert!(!page.has_more);
    }

    #[test]
    fn the_summary_prefers_a_russian_title_and_falls_back() {
        // The grid shows one name, and the choice is the client's, not the
        // user's: Russian, then romaji, then english, then the native one.
        let c = conn();
        seed(&c);
        c.execute("UPDATE anime SET title_russian = 'Атака Титанов' WHERE uid = 'al:1'", [])
            .unwrap();
        c.execute(
            "UPDATE anime SET title_romaji = NULL, title_english = 'Demon Slayer' WHERE uid = 'al:3'",
            [],
        )
        .unwrap();
        c.execute(
            "UPDATE anime SET title_romaji = NULL, title_english = NULL WHERE uid = 'al:2'",
            [],
        )
        .unwrap();
        let page = query_list(&c, &q(json!({ "per_page": 10 })), false, None, 48).unwrap();
        let by_uid: BTreeMap<&str, &AnimeSummary> =
            page.items.iter().map(|i| (i.uid.as_str(), i)).collect();
        assert_eq!(by_uid["al:1"].title, "Атака Титанов");
        assert_eq!(by_uid["al:2"].title, "Без названия", "у al:2 не осталось ни одного названия");
        assert_eq!(by_uid["al:3"].title, "Demon Slayer");
        // The dropped titles are still in the response for the detail page.
        assert_eq!(by_uid["al:3"].title_romaji, None);
    }

    #[test]
    fn a_titleless_row_still_renders() {
        // Long-tail `sh:` rows can have no title at all; the grid shows a
        // placeholder rather than a blank card.
        let c = conn();
        c.execute("INSERT INTO anime (uid, is_adult, created_at) VALUES ('sh:1', 0, 1)", [])
            .unwrap();
        let page = query_list(&c, &q(json!({})), false, None, 48).unwrap();
        assert_eq!(page.items[0].title, "Без названия");
    }

    #[test]
    fn sorting_by_score_puts_the_best_first() {
        let c = conn();
        seed(&c);
        let page = query_list(&c, &q(json!({ "sort": "score", "per_page": 10 })), false, None, 48)
            .unwrap();
        let scores: Vec<Option<i64>> = page.items.iter().map(|i| i.score).collect();
        assert_eq!(scores, vec![Some(90), Some(84), Some(82), Some(78)]);
    }

    #[test]
    fn an_unrated_title_sorts_last() {
        let c = conn();
        seed(&c);
        c.execute("UPDATE anime SET score = NULL WHERE uid = 'al:1'", []).unwrap();
        let page = query_list(&c, &q(json!({ "sort": "score", "per_page": 10 })), false, None, 48)
            .unwrap();
        assert_eq!(page.items.last().unwrap().uid, "al:1");
    }

    #[test]
    fn the_adult_filter_excludes_and_includes() {
        let c = conn();
        seed(&c);
        let only = query_list(&c, &q(json!({ "adult": "only" })), false, None, 48).unwrap();
        assert_eq!(only.total, 1);
        assert_eq!(only.items[0].uid, "ks:4");

        let safe = query_list(&c, &q(json!({ "adult": "no" })), false, None, 48).unwrap();
        assert_eq!(safe.total, 3);
    }

    #[test]
    fn the_year_range_excludes_yearless_titles_for_real() {
        let c = conn();
        seed(&c);
        c.execute("UPDATE anime SET start_year = NULL WHERE uid = 'al:1'", []).unwrap();
        let page = query_list(&c, &q(json!({ "year_from": 2000, "year_to": 2010 })), false, None, 48)
            .unwrap();
        let uids: Vec<&str> = page.items.iter().map(|i| i.uid.as_str()).collect();
        assert_eq!(uids, vec!["ks:4"]);
    }

    #[test]
    fn search_finds_a_title_by_its_romaji_name() {
        let c = conn();
        seed(&c);
        let page = query_list(&c, &q(json!({ "q": "shingeki" })), false, None, 48).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].uid, "al:2");
    }

    #[test]
    fn search_finds_a_russian_title_under_either_index() {
        // The production path picks the index from `fts_enabled()`, so the
        // contract is "the search finds the row", not "a particular index
        // does". FTS5 folds Cyrillic case; the LIKE fallback cannot, so the
        // term here is written the way the column stores it.
        let c = conn();
        seed(&c);
        c.execute("UPDATE anime SET title_russian = 'Атака Титанов' WHERE uid = 'al:1'", [])
            .unwrap();

        let has_fts: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'anime_fts'",
                [],
                |r| r.get(0),
            )
            .unwrap();

        if has_fts > 0 {
            crate::db::rebuild_fts(&c).unwrap();
            // Lowercased on purpose: only `unicode61` can match it.
            let page = query_list(&c, &q(json!({ "q": "атака" })), true, None, 48).unwrap();
            assert_eq!(page.total, 1);
            assert_eq!(page.items[0].uid, "al:1");
        } else {
            let page = query_list(&c, &q(json!({ "q": "Атака" })), false, None, 48).unwrap();
            assert_eq!(page.total, 1);
            assert_eq!(page.items[0].uid, "al:1");
        }
    }

    // ------------------------------------------------------------ suggest

    #[test]
    fn suggest_returns_a_short_list_and_respects_its_limit() {
        let c = conn();
        seed(&c);
        let v = query_suggest(&c, "shingeki", false, 1).unwrap();
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn suggest_of_a_blank_term_is_empty() {
        let c = conn();
        seed(&c);
        assert!(query_suggest(&c, "  ", false, 8).unwrap().is_empty());
    }

    #[test]
    fn suggest_prefers_the_russian_title() {
        let c = conn();
        seed(&c);
        c.execute("UPDATE anime SET title_russian = 'Атака Титанов' WHERE uid = 'al:1'", [])
            .unwrap();
        let v = query_suggest(&c, "Титанов", false, 8).unwrap();
        assert_eq!(v[0].title, "Атака Титанов");
        assert_eq!(v[0].uid, "al:1");
    }

    #[test]
    fn suggest_clamps_its_limit() {
        let c = conn();
        for i in 0..30 {
            insert_anime(&c, &format!("al:{}", i), Some(&format!("Title {}", i)));
        }
        // A limit of 1000 would hand the client the whole catalogue on every
        // keystroke.
        assert_eq!(query_suggest(&c, "title", false, 1000).unwrap().len(), 20);
        assert_eq!(query_suggest(&c, "title", false, 5).unwrap().len(), 5);
    }
}
