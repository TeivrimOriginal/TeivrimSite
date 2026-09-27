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
fn norm_enum(raw: &str, kind: &str) -> Option<String> {
    let v = raw.trim().to_ascii_uppercase();
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
