//! Watchlist / favourites.
//!
//! One row per (user, anime). `status` is the list bucket and `is_favorite` is
//! a separate star, because "plan to watch" and "favourite" are different
//! intentions and people use them differently.

use crate::db::Handle;
use crate::error::{now_ts, ApiError, ApiResult};
use crate::models::*;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct UpsertBody {
    pub uid: String,
    pub status: Option<String>,
    pub is_favorite: Option<bool>,
    /// 1..10, or null to clear.
    pub score: Option<Option<i64>>,
    pub progress: Option<Option<i64>>,
    pub notes: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
pub struct ListFilter {
    pub status: Option<String>,
    /// `1` restricts to starred entries.
    pub favorites: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}


pub fn upsert(db: &Handle, user_id: i64, body: UpsertBody) -> ApiResult<LibraryEntry> {
    if body.uid.is_empty() || body.uid.len() > 64 {
        return Err(ApiError::bad("Некорректный uid"));
    }
    if let Some(s) = &body.status {
        if !crate::api::catalog::VALID_STATUSES.contains(&s.as_str()) {
            return Err(ApiError::bad(format!(
                "Неизвестный статус «{}». Допустимо: {}",
                s,
                crate::api::catalog::VALID_STATUSES.join(", ")
            )));
        }
    }
    if let Some(Some(score)) = body.score {
        if !(1..=10).contains(&score) {
            return Err(ApiError::bad("Оценка должна быть от 1 до 10"));
        }
    }

    let conn = db.conn().map_err(|e| ApiError::internal(e.to_string()))?;

    // Refuse to track something that is not in the catalogue, otherwise a typo
    // silently creates an entry that can never be displayed.
    let exists: bool = conn
        .query_row("SELECT 1 FROM anime WHERE uid = ?1", params![body.uid], |_| Ok(true))
        .optional()
        .map_err(ApiError::from)?
        .unwrap_or(false);
    if !exists {
        return Err(ApiError::NotFound(format!("Аниме {} не найдено", body.uid)));
    }

    let episodes: Option<i64> = conn
        .query_row("SELECT episodes FROM anime WHERE uid = ?1", params![body.uid], |r| {
            r.get(0)
        })
        .optional()
        .map_err(ApiError::from)?
        .flatten();

    let now = now_ts();

    // PATCH semantics: absent means "leave as is", explicit null means "clear".
    // The three flags are what distinguish the two in the upsert below.
    let set_score = body.score.is_some();
    let set_progress = body.progress.is_some();
    let set_notes = body.notes.is_some();
    let score = body.score.flatten();
    let progress = body.progress.flatten();
    let notes = body.notes.flatten();

    conn.execute(
        "INSERT INTO favorites (user_id, uid, status, is_favorite, score, progress, episodes, notes, created_at, updated_at)
         VALUES (?1, ?2, COALESCE(?3, 'planned'), COALESCE(?4, 0), ?5, ?6, ?7, ?8, ?9, ?9)
         ON CONFLICT(user_id, uid) DO UPDATE SET
            status      = COALESCE(?3, favorites.status),
            is_favorite = COALESCE(?4, favorites.is_favorite),
            score       = CASE WHEN ?10 THEN ?5 ELSE favorites.score END,
            progress    = CASE WHEN ?11 THEN ?6 ELSE favorites.progress END,
            notes       = CASE WHEN ?12 THEN ?8 ELSE favorites.notes END,
            episodes    = ?7,
            updated_at  = ?9",
        params![
            user_id,
            body.uid,
            body.status,
            body.is_favorite.map(|b| if b { 1 } else { 0 }),
            score,
            progress,
            episodes,
            notes,
            now,
            set_score,
            set_progress,
            set_notes,
        ],
    )?;

    crate::api::detail::load_library_entry(&conn, user_id, &body.uid)?
        .ok_or_else(|| ApiError::internal("запись не найдена после вставки"))
}

pub fn remove(db: &Handle, user_id: i64, uid: &str) -> ApiResult<()> {
    let conn = db.conn().map_err(|e| ApiError::internal(e.to_string()))?;
    let n = conn.execute(
        "DELETE FROM favorites WHERE user_id = ?1 AND uid = ?2",
        params![user_id, uid],
    )?;
    if n == 0 {
        return Err(ApiError::NotFound("Записи нет в списке".into()));
    }
    Ok(())
}


/// Per-status counts for the watchlist tabs.
#[derive(Debug, Serialize)]
pub struct Counts {
    pub watching: i64,
    pub planned: i64,
    pub completed: i64,
    pub dropped: i64,
    pub favorites: i64,
    pub total: i64,
}

pub fn counts(conn: &Connection, user_id: i64) -> ApiResult<Counts> {
    let row: (i64, i64, i64, i64, i64, i64) = conn.query_row(
        "SELECT
            COALESCE(SUM(status = 'watching'), 0),
            COALESCE(SUM(status = 'planned'), 0),
            COALESCE(SUM(status = 'completed'), 0),
            COALESCE(SUM(status = 'dropped'), 0),
            COALESCE(SUM(is_favorite = 1), 0),
            COUNT(*)
         FROM favorites WHERE user_id = ?1",
        params![user_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
    )?;
    Ok(Counts {
        watching: row.0,
        planned: row.1,
        completed: row.2,
        dropped: row.3,
        favorites: row.4,
        total: row.5,
    })
}

/// The watchlist joined to catalogue metadata, so a list screen needs one
/// request instead of N.
#[derive(Debug, Serialize)]
pub struct ListEntry {
    #[serde(flatten)]
    pub item: AnimeSummary,
    pub library: LibraryEntry,
}

pub fn list_with_anime(
    conn: &Connection,
    user_id: i64,
    filter: &ListFilter,
) -> ApiResult<Vec<ListEntry>> {
    // The watchlist columns have to be in the projection too: the row mapper
    // reads them right after the summary columns, and an out-of-range index
    // would make every row fail and the list come back empty.
    let mut sql = format!(
        "SELECT {}, f.status, f.is_favorite, f.score, f.progress, f.episodes, f.notes, f.updated_at \
         FROM favorites f JOIN anime a ON a.uid = f.uid WHERE f.user_id = ?1",
        crate::api::catalog::summary_columns()
    );
    let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(user_id)];
    if let Some(s) = filter.status.as_deref().filter(|s| !s.is_empty()) {
        sql.push_str(" AND f.status = ?");
        values.push(Box::new(s.to_string()));
    }
    if matches!(filter.favorites.as_deref(), Some("1" | "true" | "yes")) {
        sql.push_str(" AND f.is_favorite = 1");
    }
    sql.push_str(" ORDER BY f.is_favorite DESC, f.updated_at DESC LIMIT ? OFFSET ?");
    values.push(Box::new(filter.limit.unwrap_or(200).clamp(1, 1000)));
    values.push(Box::new(filter.offset.unwrap_or(0).max(0)));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(values.iter()), |r| {
        let item = crate::api::catalog::summary_from_row(r)?;
        let library = LibraryEntry {
            uid: item.uid.clone(),
            status: r.get(21)?,
            is_favorite: r.get::<_, i64>(22)? == 1,
            score: r.get(23)?,
            progress: r.get(24)?,
            episodes: r.get(25)?,
            notes: r.get(26)?,
            updated_at: r.get(27)?,
        };
        Ok(ListEntry { item, library })
    })?;

    // Surface row errors instead of dropping them: a silent empty list is
    // indistinguishable from "you have nothing saved".
    let mut out = Vec::new();
    for r in rows {
        match r {
            Ok(v) => out.push(v),
            Err(e) => {
                crate::error::log_error(&format!("[api] строка списка пропущена: {}", e));
            }
        }
    }
    Ok(out)
}
