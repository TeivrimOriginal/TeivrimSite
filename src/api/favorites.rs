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
    /// 1..10. `None` = leave alone, `Some(None)` = clear.
    #[serde(default, deserialize_with = "double_option")]
    pub score: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub progress: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub notes: Option<Option<String>>,
}

/// Tells "field absent" from "field is null".
///
/// A plain `Option<Option<T>>` collapses both to `None`, which makes it
/// impossible for a client to clear a score it has already set. The trick is
/// that `#[serde(default)]` handles the absent case before this function is
/// ever called, so here `T` is really an `Option<Inner>`: deserializing the
/// present value yields `None` for a JSON `null` and `Some(x)` otherwise, and
/// re-wrapping in `Some` restores the distinction.
fn double_option<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

#[derive(Debug, Default, Deserialize)]
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

/// The `WHERE` of the watchlist query, with the values that go with it.
///
/// The list and the count have to agree exactly — a total that does not match
/// the rows it counts is how a client ends up showing "12 of 12" and then
/// stopping, or an empty page that claims there is more.
fn filter_clause(user_id: i64, filter: &ListFilter) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut sql = String::from(" WHERE f.user_id = ?");
    let mut values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(user_id)];
    if let Some(s) = filter.status.as_deref().filter(|s| !s.is_empty()) {
        sql.push_str(" AND f.status = ?");
        values.push(Box::new(s.to_string()));
    }
    if matches!(filter.favorites.as_deref(), Some("1" | "true" | "yes")) {
        sql.push_str(" AND f.is_favorite = 1");
    }
    (sql, values)
}

pub fn list_with_anime(
    conn: &Connection,
    user_id: i64,
    filter: &ListFilter,
) -> ApiResult<Paged<ListEntry>> {
    // The envelope is the same `Paged` the catalogue answers with, and it has
    // to be: the frontend renders both through one code path that reads
    // `items` and `total`. Answering a bare array here made the watchlist tab
    // read `undefined` for both and render as empty no matter what was saved.
    let limit = filter.limit.unwrap_or(200).clamp(1, 1000);
    let offset = filter.offset.unwrap_or(0).max(0);
    let (where_, values) = filter_clause(user_id, filter);

    let total: i64 = {
        let sql = format!("SELECT COUNT(*) FROM favorites f{}", where_);
        conn.query_row(&sql, rusqlite::params_from_iter(values.iter()), |r| r.get(0))?
    };

    // The watchlist columns have to be in the projection too: the row mapper
    // reads them right after the summary columns, and an out-of-range index
    // would make every row fail and the list come back empty.
    let mut sql = format!(
        "SELECT {}, f.status, f.is_favorite, f.score, f.progress, f.episodes, f.notes, f.updated_at \
         FROM favorites f JOIN anime a ON a.uid = f.uid{}",
        crate::api::catalog::summary_columns(),
        where_
    );
    sql.push_str(" ORDER BY f.is_favorite DESC, f.updated_at DESC LIMIT ? OFFSET ?");
    let mut all = values;
    all.push(Box::new(limit));
    all.push(Box::new(offset));

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(all.iter()), |r| {
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
    let mut items = Vec::new();
    for r in rows {
        match r {
            Ok(v) => items.push(v),
            Err(e) => {
                crate::error::log_error(&format!("[api] строка списка пропущена: {}", e));
            }
        }
    }

    let total_pages = if limit > 0 { (total + limit - 1) / limit } else { 0 };
    let has_more = items.len() as i64 == limit && offset + limit < total;
    // One-based, the same convention `Paged` uses for the catalogue.
    let page = u32::try_from(offset / limit + 1).unwrap_or(1);
    Ok(Paged {
        items,
        page,
        per_page: limit,
        total,
        total_pages,
        has_more,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::detail::load_library_entry;
    use crate::db::testing::test_db;
    use actix_web::http::StatusCode;
    use actix_web::ResponseError;
    use serde_json::json;

    fn seed(test_db: &crate::db::testing::TestDb) -> i64 {
        {
            let c = test_db.conn();
            c.execute("INSERT INTO anime (uid, episodes, is_adult, created_at) VALUES ('al:1', 25, 0, 1)", [])
                .unwrap();
            c.execute("INSERT INTO anime (uid, episodes, is_adult, created_at) VALUES ('al:2', 12, 0, 1)", [])
                .unwrap();
            c.execute("INSERT INTO users (username, username_key, password_hash, created_at) VALUES ('u','u','h',1)", [])
                .unwrap();
        }
        test_db
            .conn()
            .query_row("SELECT id FROM users", [], |r| r.get(0))
            .unwrap()
    }

    fn up(uid: &str) -> UpsertBody {
        UpsertBody {
            uid: uid.to_string(),
            status: None,
            is_favorite: None,
            score: None,
            progress: None,
            notes: None,
        }
    }

    // ------------------------------------------------- the double Option

    #[test]
    fn an_absent_field_is_left_alone_and_a_null_clears_it() {
        // PATCH semantics in one test: the three states have to stay
        // distinguishable, which is the entire reason for `Option<Option<T>>`.
        let absent: UpsertBody = serde_json::from_value(json!({ "uid": "al:1" })).unwrap();
        assert_eq!(absent.score, None);
        assert_eq!(absent.progress, None);
        assert_eq!(absent.notes, None);

        let cleared: UpsertBody =
            serde_json::from_value(json!({ "uid": "al:1", "score": null, "notes": null })).unwrap();
        assert_eq!(cleared.score, Some(None));
        assert_eq!(cleared.notes, Some(None));
        assert_eq!(cleared.progress, None);

        let set: UpsertBody =
            serde_json::from_value(json!({ "uid": "al:1", "score": 8, "notes": "ok" })).unwrap();
        assert_eq!(set.score, Some(Some(8)));
        assert_eq!(set.notes, Some(Some("ok".to_string())));
    }

    #[test]
    fn a_plain_option_would_not_be_able_to_clear_a_value() {
        // The counter-example, stated as a test: with `Option<i64>` both
        // documents below parse to None and "clear the score" becomes
        // impossible to express.
        let cleared: UpsertBody = serde_json::from_value(json!({ "uid": "al:1", "score": null })).unwrap();
        let absent: UpsertBody = serde_json::from_value(json!({ "uid": "al:1" })).unwrap();
        assert_ne!(cleared.score, absent.score);
    }

    // ---------------------------------------------------------- validation

    #[test]
    fn a_malformed_uid_is_refused() {
        let db = test_db();
        let uid = seed(&db);
        let e = upsert(&db.handle, uid, up("")).unwrap_err();
        assert_eq!(e.status_code(), StatusCode::BAD_REQUEST);
        let long = "x".repeat(65);
        let e = upsert(&db.handle, uid, up(&long)).unwrap_err();
        assert_eq!(e.status_code(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn an_unknown_status_is_refused_and_lists_the_valid_ones() {
        // A silently accepted status leaves the entry invisible in every tab.
        let db = test_db();
        let uid = seed(&db);
        let mut b = up("al:1");
        b.status = Some("watchingg".into());
        let e = upsert(&db.handle, uid, b).unwrap_err();
        assert_eq!(e.status_code(), StatusCode::BAD_REQUEST);
        assert!(e.to_string().contains("watching"), "{}", e);
    }

    #[test]
    fn every_known_status_is_accepted() {
        let db = test_db();
        let uid = seed(&db);
        for s in crate::api::catalog::VALID_STATUSES {
            let mut b = up("al:1");
            b.status = Some((*s).to_string());
            let entry = upsert(&db.handle, uid, b).unwrap();
            assert_eq!(&entry.status, s);
        }
    }

    #[test]
    fn a_score_outside_one_to_ten_is_refused() {
        let db = test_db();
        let uid = seed(&db);
        for bad in [0, 11, -1] {
            let mut b = up("al:1");
            b.score = Some(Some(bad));
            let e = upsert(&db.handle, uid, b).unwrap_err();
            assert_eq!(e.status_code(), StatusCode::BAD_REQUEST, "оценка {}", bad);
        }
    }

    #[test]
    fn a_title_that_is_not_in_the_catalogue_is_refused() {
        // Otherwise a typo silently creates an entry that can never be shown.
        let db = test_db();
        let uid = seed(&db);
        let e = upsert(&db.handle, uid, up("al:9999")).unwrap_err();
        assert_eq!(e.status_code(), StatusCode::NOT_FOUND);
    }

    // -------------------------------------------------------------- upsert

    #[test]
    fn a_first_add_lands_in_planned() {
        let db = test_db();
        let uid = seed(&db);
        let e = upsert(&db.handle, uid, up("al:1")).unwrap();
        assert_eq!(e.uid, "al:1");
        assert_eq!(e.status, "planned");
        assert!(!e.is_favorite);
        // The episode count is copied from the catalogue so the progress bar has
        // something to divide by.
        assert_eq!(e.episodes, Some(25));
    }

    #[test]
    fn an_absent_field_leaves_the_stored_value_alone() {
        let db = test_db();
        let uid = seed(&db);
        let mut first = up("al:1");
        first.score = Some(Some(9));
        first.progress = Some(Some(5));
        first.notes = Some(Some("заметка".into()));
        upsert(&db.handle, uid, first).unwrap();

        let mut second = up("al:1");
        second.status = Some("watching".into());
        let e = upsert(&db.handle, uid, second).unwrap();
        assert_eq!(e.status, "watching");
        assert_eq!(e.score, Some(9));
        assert_eq!(e.progress, Some(5));
        assert_eq!(e.notes.as_deref(), Some("заметка"));
    }

    #[test]
    fn an_explicit_null_clears_the_stored_value() {
        let db = test_db();
        let uid = seed(&db);
        let mut first = up("al:1");
        first.score = Some(Some(9));
        first.notes = Some(Some("заметка".into()));
        upsert(&db.handle, uid, first).unwrap();

        let mut second = up("al:1");
        second.score = Some(None);
        second.notes = Some(None);
        let e = upsert(&db.handle, uid, second).unwrap();
        assert_eq!(e.score, None);
        assert_eq!(e.notes, None);
    }

    #[test]
    fn the_episode_count_is_refreshed_from_the_catalogue() {
        // A running series gets more episodes between sessions; the stored copy
        // has to follow or the progress bar stops moving.
        let db = test_db();
        let uid = seed(&db);
        upsert(&db.handle, uid, up("al:1")).unwrap();
        db.conn().execute("UPDATE anime SET episodes = 30 WHERE uid = 'al:1'", []).unwrap();
        let e = upsert(&db.handle, uid, up("al:1")).unwrap();
        assert_eq!(e.episodes, Some(30));
    }

    #[test]
    fn the_favourite_star_is_independent_of_the_status() {
        // "Plan to watch" and "favourite" are different intentions, so the star
        // is a separate flag.
        let db = test_db();
        let uid = seed(&db);
        let mut b = up("al:1");
        b.is_favorite = Some(true);
        let e = upsert(&db.handle, uid, b).unwrap();
        assert!(e.is_favorite);
        assert_eq!(e.status, "planned");
    }

    // ------------------------------------------------------------- remove

    #[test]
    fn removing_an_entry_works_once() {
        let db = test_db();
        let uid = seed(&db);
        upsert(&db.handle, uid, up("al:1")).unwrap();
        remove(&db.handle, uid, "al:1").unwrap();
        let e = remove(&db.handle, uid, "al:1").unwrap_err();
        assert_eq!(e.status_code(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn removing_someone_elses_entry_is_a_not_found() {
        // The delete is scoped by user_id, so one account cannot clear another.
        let db = test_db();
        let uid = seed(&db);
        upsert(&db.handle, uid, up("al:1")).unwrap();
        let e = remove(&db.handle, 999, "al:1").unwrap_err();
        assert_eq!(e.status_code(), StatusCode::NOT_FOUND);
        assert!(load_library_entry(&db.conn(), uid, "al:1").unwrap().is_some());
    }

    // ------------------------------------------------------------- counts

    #[test]
    fn counts_cover_every_tab() {
        let db = test_db();
        let uid = seed(&db);
        for (u, status, fav) in [
            ("al:1", Some("watching"), Some(true)),
            ("al:2", Some("watching"), Some(false)),
            // No star in this body: PATCH semantics mean "leave as is", which is
            // the only way the star survives a second edit of the same entry.
            ("al:1", Some("completed"), None),
        ] {
            let mut b = up(u);
            b.status = status.map(|s| s.to_string());
            b.is_favorite = fav;
            upsert(&db.handle, uid, b).unwrap();
        }
        let c = counts(&db.conn(), uid).unwrap();
        assert_eq!(c.watching, 1, "al:1 переехал в completed вторым проходом");
        assert_eq!(c.planned, 0);
        assert_eq!(c.completed, 1);
        assert_eq!(c.dropped, 0);
        assert_eq!(c.favorites, 1);
        assert_eq!(c.total, 2);
    }

    #[test]
    fn the_counts_of_an_empty_list_are_all_zero() {
        // The tabs render from these numbers; NULL would break the arithmetic.
        let db = test_db();
        let uid = seed(&db);
        let c = counts(&db.conn(), uid).unwrap();
        assert_eq!((c.watching, c.planned, c.completed, c.dropped, c.favorites, c.total), (0, 0, 0, 0, 0, 0));
    }

    // --------------------------------------------------------------- list

    fn filter(status: Option<&str>, favorites: Option<&str>, limit: Option<i64>, offset: Option<i64>) -> ListFilter {
        ListFilter {
            status: status.map(|s| s.to_string()),
            favorites: favorites.map(|s| s.to_string()),
            limit,
            offset,
        }
    }

    #[test]
    fn the_list_joins_the_catalogue_metadata() {
        // One request instead of N: the watchlist screen needs a title and a
        // cover for every row.
        let db = test_db();
        let uid = seed(&db);
        db.conn()
            .execute("UPDATE anime SET title_romaji = 'Shingeki no Kyojin' WHERE uid = 'al:1'", [])
            .unwrap();
        upsert(&db.handle, uid, up("al:1")).unwrap();

        let page = list_with_anime(&db.conn(), uid, &ListFilter::default()).unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].item.uid, "al:1");
        assert_eq!(page.items[0].item.title, "Shingeki no Kyojin");
        assert_eq!(page.items[0].library.status, "planned");
    }

    #[test]
    fn the_list_answers_with_the_same_envelope_as_the_catalogue() {
        // The bug this pins down: the endpoint used to answer with a bare
        // array. The frontend renders the watchlist and the catalogue through
        // one code path that reads `items` and `total`, so on an array both
        // were `undefined` and the watchlist always rendered as empty.
        let db = test_db();
        let uid = seed(&db);
        upsert(&db.handle, uid, up("al:1")).unwrap();

        let v = serde_json::to_value(
            list_with_anime(&db.conn(), uid, &ListFilter::default()).unwrap(),
        )
        .unwrap();
        assert!(v.is_object(), "ответ должен быть объектом, а не массивом: {}", v);
        for key in ["items", "page", "per_page", "total", "total_pages", "has_more"] {
            assert!(v.get(key).is_some(), "нет поля {} в {}", key, v);
        }
        assert_eq!(v["items"][0]["library"]["uid"], "al:1");
    }

    #[test]
    fn an_empty_watchlist_is_an_empty_page_and_not_a_missing_one() {
        // `items: []` with `total: 0` is what the client turns into "you have
        // nothing saved". Anything else and it either shows a spinner forever
        // or claims there is more.
        let db = test_db();
        let uid = seed(&db);
        let v = serde_json::to_value(
            list_with_anime(&db.conn(), uid, &ListFilter::default()).unwrap(),
        )
        .unwrap();
        assert_eq!(v["items"], json!([]));
        assert_eq!(v["total"], 0);
        assert_eq!(v["total_pages"], 0);
        assert_eq!(v["has_more"], false);
    }

    #[test]
    fn the_total_counts_the_filtered_list_and_not_the_whole_watchlist() {
        // A total that ignores the filter is how a client ends up paging
        // through empty pages looking for entries it already showed.
        let db = test_db();
        let uid = seed(&db);
        let mut a = up("al:1");
        a.status = Some("watching".into());
        upsert(&db.handle, uid, a).unwrap();
        upsert(&db.handle, uid, up("al:2")).unwrap();

        let page = list_with_anime(&db.conn(), uid, &filter(Some("watching"), None, None, None)).unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.total, 1, "всего в списке две записи");

        let all = list_with_anime(&db.conn(), uid, &filter(None, None, None, None)).unwrap();
        assert_eq!(all.total, 2);
    }

    #[test]
    fn the_page_numbers_follow_the_offset() {
        // The client pages by offset, so the envelope has to say where it is
        // in the list rather than always claiming page one.
        let db = test_db();
        let uid = seed(&db);
        for u in ["al:1", "al:2"] {
            upsert(&db.handle, uid, up(u)).unwrap();
        }
        let first = list_with_anime(&db.conn(), uid, &filter(None, None, Some(1), Some(0))).unwrap();
        assert_eq!((first.page, first.per_page, first.total_pages, first.has_more), (1, 1, 2, true));

        let second = list_with_anime(&db.conn(), uid, &filter(None, None, Some(1), Some(1))).unwrap();
        assert_eq!((second.page, second.has_more), (2, false));
        assert_ne!(first.items[0].item.uid, second.items[0].item.uid);
    }

    #[test]
    fn the_list_can_be_filtered_by_status_and_by_star() {
        let db = test_db();
        let uid = seed(&db);
        let mut a = up("al:1");
        a.status = Some("watching".into());
        a.is_favorite = Some(true);
        upsert(&db.handle, uid, a).unwrap();
        upsert(&db.handle, uid, up("al:2")).unwrap();

        let page = list_with_anime(&db.conn(), uid, &filter(Some("watching"), None, None, None)).unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].item.uid, "al:1");

        let starred = list_with_anime(&db.conn(), uid, &filter(None, Some("1"), None, None)).unwrap();
        assert_eq!(starred.items.len(), 1);
    }

    #[test]
    fn the_list_of_another_user_is_empty() {
        let db = test_db();
        let uid = seed(&db);
        upsert(&db.handle, uid, up("al:1")).unwrap();
        let page = list_with_anime(&db.conn(), 999, &ListFilter::default()).unwrap();
        assert!(page.items.is_empty());
        assert_eq!(page.total, 0);
    }

    #[test]
    fn the_list_limit_is_clamped() {
        let db = test_db();
        let uid = seed(&db);
        upsert(&db.handle, uid, up("al:1")).unwrap();
        // A negative offset would make SQLite read from the end of the table,
        // and an unbounded limit would let one request pull the whole list.
        let page = list_with_anime(&db.conn(), uid, &filter(None, None, Some(100_000), Some(-5))).unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.per_page, 1_000);
    }
}
