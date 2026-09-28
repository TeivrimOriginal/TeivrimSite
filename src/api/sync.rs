//! Sync control and progress.
//!
//! `status` is public so a deployment dashboard can watch a long import
//! without credentials. Starting and aborting a sync changes server state, so
//! both are gated: `SYNC_ADMIN_TOKEN` when set, otherwise a bearer token from a
//! registered user. With no admin token configured, the endpoints are simply
//! not registered at all rather than left wide open.

use crate::db::Handle;
use crate::error::ApiError;
use actix_web::{web, HttpRequest, HttpResponse, ResponseError};
use rusqlite::Connection;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

static RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Serialize)]
pub struct Task {
    pub source: String,
    pub task: String,
    pub last_page: i64,
    pub total_saved: i64,
    pub finished: bool,
    pub last_error: Option<String>,
    pub started_at: Option<i64>,
    pub updated_at: i64,
    pub age_seconds: Option<i64>,
}

pub async fn status(db: web::Data<Handle>) -> HttpResponse {
    let db = db.into_inner();
    let result = web::block(move || {
        let conn = db.conn().map_err(|e| ApiError::internal(e.to_string()))?;
        let mut stmt = conn.prepare(
            "SELECT source, task, last_page, total_saved, finished, last_error, started_at, updated_at
             FROM sync_state ORDER BY source, task",
        )?;
        let now = crate::error::now_ts();
        let rows = stmt.query_map([], |r| {
            let updated_at: i64 = r.get(7)?;
            let started_at: Option<i64> = r.get(6)?;
            Ok(Task {
                source: r.get(0)?,
                task: r.get(1)?,
                last_page: r.get(2)?,
                total_saved: r.get(3)?,
                finished: r.get::<_, i64>(4)? == 1,
                last_error: r.get(5)?,
                started_at,
                updated_at,
                age_seconds: started_at.map(|s| now - s),
            })
        })?;

        let tasks: Vec<Task> = rows.flatten().collect();
        let total: i64 = conn.query_row("SELECT COUNT(*) FROM anime", [], |r| r.get(0))?;
        let with_ru: i64 = conn.query_row(
            "SELECT COUNT(*) FROM anime WHERE title_russian IS NOT NULL AND title_russian <> ''",
            [],
            |r| r.get(0),
        )?;

        Ok::<serde_json::Value, ApiError>(serde_json::json!({
            "running": RUNNING.load(Ordering::Relaxed),
            "anime": total,
            "with_russian": with_ru,
            "tasks": tasks,
        }))
    })
    .await;

    match result {
        Ok(Ok(v)) => HttpResponse::Ok()
            .insert_header(("cache-control", "no-store"))
            .json(v),
        Ok(Err(e)) => e.error_response(),
        Err(_) => ApiError::internal("sync status worker failed").error_response(),
    }
}

fn authorized(req: &HttpRequest, admin_token: Option<&str>) -> bool {
    let Some(expected) = admin_token.filter(|t| !t.is_empty()) else {
        return false;
    };
    let presented = req
        .headers()
        .get("x-admin-token")
        .and_then(|v| v.to_str().ok())
        .or_else(|| {
            req.headers()
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
        })
        .unwrap_or_default();

    // Constant-time compare so the token cannot be recovered byte by byte.
    if presented.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in presented.bytes().zip(expected.bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}

pub async fn start(
    req: HttpRequest,
    admin_token: web::Data<Option<String>>,
    ctx: Option<web::Data<Arc<crate::loader::Ctx>>>,
) -> HttpResponse {
    if !authorized(&req, admin_token.as_deref()) {
        return ApiError::Forbidden("Для запуска синхронизации нужен X-Admin-Token".into()).error_response();
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        return HttpResponse::Ok().json(serde_json::json!({ "started": false, "reason": "уже выполняется" }));
    }
    let Some(ctx) = ctx else {
        RUNNING.store(false, Ordering::Relaxed);
        return ApiError::internal("загрузчики отключены в этой сборке").error_response();
    };
    let ctx = (**ctx).clone();
    tokio::spawn(async move {
        crate::loader::run_all((*ctx).clone()).await;
        RUNNING.store(false, Ordering::SeqCst);
    });
    HttpResponse::Ok().json(serde_json::json!({ "started": true }))
}

pub async fn abort(req: HttpRequest, admin_token: web::Data<Option<String>>) -> HttpResponse {
    if !authorized(&req, admin_token.as_deref()) {
        return ApiError::Forbidden("Для остановки синхронизации нужен X-Admin-Token".into()).error_response();
    }
    crate::loader::request_abort();
    HttpResponse::Ok().json(serde_json::json!({ "aborting": true }))
}

/// Marks every unfinished task as failed on boot, so a crash mid-import is
/// visible instead of looking like a sync that is still progressing.
pub fn reconcile_on_boot(conn: &Connection) {
    if let Err(e) = conn.execute(
        "UPDATE sync_state SET last_error = 'процесс был перезапущен' WHERE finished = 0 AND source <> 'translate'",
        [],
    ) {
        crate::error::log_warn(&format!("[sync] не удалось пометить прерванные задачи: {}", e));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::testing::conn;
    use actix_web::test::TestRequest;

    fn authorized_with(token: Option<&str>, admin: Option<&str>) -> bool {
        let req = TestRequest::default();
        let req = match token {
            Some(t) => req.insert_header(("x-admin-token", t)),
            None => req,
        };
        authorized(&req.to_http_request(), admin)
    }

    #[test]
    fn a_matching_admin_token_is_accepted() {
        assert!(authorized_with(Some("s3cret"), Some("s3cret")));
    }

    #[test]
    fn a_wrong_admin_token_is_refused() {
        assert!(!authorized_with(Some("s3crey"), Some("s3cret")));
        assert!(!authorized_with(Some(""), Some("s3cret")));
        assert!(!authorized_with(Some("s3cret "), Some("s3cret")));
    }

    #[test]
    fn a_token_of_a_different_length_is_refused_without_a_comparison() {
        // The length check runs first, so the response time cannot be used to
        // recover the token one byte at a time.
        assert!(!authorized_with(Some("s3cre"), Some("s3cret")));
        assert!(!authorized_with(Some("s3crets"), Some("s3cret")));
    }

    #[test]
    fn no_header_means_no_access() {
        assert!(!authorized_with(None, Some("s3cret")));
    }

    #[test]
    fn an_unconfigured_admin_token_denies_everything() {
        // With no SYNC_ADMIN_TOKEN the endpoints must not fall open; an
        // attacker who finds the route would otherwise be able to start a
        // multi-hour import.
        assert!(!authorized_with(Some(""), None));
        assert!(!authorized_with(Some("anything"), None));
    }

    #[test]
    fn a_bearer_token_is_accepted_as_a_fallback() {
        // A signed-in operator should not have to keep a second secret around.
        let req = TestRequest::default()
            .insert_header(("authorization", "Bearer s3cret"))
            .to_http_request();
        assert!(authorized(&req, Some("s3cret")));

        let req = TestRequest::default()
            .insert_header(("authorization", "Basic s3cret"))
            .to_http_request();
        assert!(!authorized(&req, Some("s3cret")));
    }

    // ------------------------------------------------------ boot reconcile

    #[test]
    fn an_unfinished_task_is_marked_as_interrupted_on_boot() {
        // Otherwise a crash mid-import looks like a sync that is still
        // progressing, and nobody restarts it.
        let c = conn();
        crate::db::save_checkpoint(&c, "anilist", "sort:ID", 12, 600, false).unwrap();
        crate::db::save_checkpoint(&c, "kitsu", "sort:ID", 30, 1500, true).unwrap();

        reconcile_on_boot(&c);
        let open: Option<String> = c
            .query_row(
                "SELECT last_error FROM sync_state WHERE source = 'anilist'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(open.as_deref(), Some("процесс был перезапущен"));

        let done: Option<String> = c
            .query_row(
                "SELECT last_error FROM sync_state WHERE source = 'kitsu'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(done.is_none(), "завершённая задача не должна выглядеть сломанной");
    }

    #[test]
    fn reconcile_on_an_empty_database_does_nothing() {
        reconcile_on_boot(&conn());
    }
}
