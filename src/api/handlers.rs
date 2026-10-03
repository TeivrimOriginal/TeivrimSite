//! HTTP handlers. Every SQLite call happens inside `web::block` so the
//! blocking driver never runs on an async worker thread.

use super::{auth, catalog, detail, favorites};
use crate::db::Handle;
use crate::error::{ApiError, ApiResult};
use crate::http::ratelimit::Limiter;
use crate::models::*;
use actix_web::{web, HttpRequest, HttpResponse, ResponseError};

/// Limits an endpoint, returning 429 with `Retry-After` when exceeded.
///
/// The client address has to be resolved before `web::block` because
/// `ServiceRequest` is `!Send`; the limiter itself is only touched by its own
/// short-lived `check`.
// `HttpResponse` is a fat type, so this `Result` has a large `Err` variant. It is
// boxed nowhere on purpose: the success path stays a `()`, and the error path
// already builds a response, so an extra allocation would buy nothing.
#[allow(clippy::result_large_err)]
async fn limited(
    limiter: &Limiter,
    req: &HttpRequest,
    budget: u32,
) -> Result<(), actix_web::HttpResponse> {
    let key = crate::http::client_key_from(req);
    let decision = limiter.check_n(&key, budget);
    if decision.allowed {
        return Ok(());
    }
    Err(HttpResponse::TooManyRequests()
        .insert_header(("retry-after", decision.retry_after.to_string()))
        .json(serde_json::json!({
            "error": {
                "code": "rate_limited",
                "message": "Слишком много запросов, попробуйте позже"
            }
        })))
}

/// Runs a blocking DB closure off the async worker threads.
async fn db_block<T, F>(f: F) -> ApiResult<T>
where
    F: FnOnce() -> ApiResult<T> + Send + 'static,
    T: Send + 'static,
{
    match web::block(f).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e),
        Err(e) => {
            crate::error::log_error(&format!("web::block: {}", e));
            Err(ApiError::internal("internal worker failure"))
        }
    }
}

/// Extracts the bearer token on the async side. `HttpRequest` is `!Send`
/// (it holds an `Rc`), so only the plain string may cross into `web::block`.
fn token_of(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

fn authenticate(db: &Handle, token: Option<&str>) -> ApiResult<i64> {
    auth::authenticate(db, token).ok_or_else(|| ApiError::Unauthorized("Требуется вход".into()))
}

// ==================================================================
// Catalogue
// ==================================================================

pub async fn list(
    db: web::Data<Handle>,
    max_per_page: web::Data<i64>,
    req: HttpRequest,
    q: web::Query<ListQuery>,
) -> HttpResponse {
    let db = db.into_inner();
    let token = token_of(&req);
    let query = q.into_inner();
    let per_page_cap = *max_per_page.get_ref();
    let fts = db.fts_enabled();

    let result = db_block(move || {
        let user_id = auth::authenticate(&db, token.as_deref());
        let conn = db.conn().map_err(|e| ApiError::internal(e.to_string()))?;
        catalog::query_list(&conn, &query, fts, user_id, per_page_cap)
    })
    .await;

    match result {
        Ok(page) => HttpResponse::Ok()
            .insert_header(("x-total-count", page.total.to_string()))
            .json(page),
        Err(e) => e.error_response(),
    }
}

pub async fn detail(
    db: web::Data<Handle>,
    path: web::Path<String>,
    req: HttpRequest,
) -> HttpResponse {
    let db = db.into_inner();
    let uid = path.into_inner();
    let token = token_of(&req);
    let fts = db.fts_enabled();

    let result = db_block(move || {
        let user_id = auth::authenticate(&db, token.as_deref());
        let conn = db.conn().map_err(|e| ApiError::internal(e.to_string()))?;
        detail::load_detail(&conn, &uid, fts, user_id)
    })
    .await;

    match result {
        Ok(d) => HttpResponse::Ok()
            // Detail pages change only when a sync touches the row, so a short
            // shared cache with stale-while-revalidate is safe and saves the
            // mobile client a round trip on back-navigation.
            .insert_header((
                "cache-control",
                "public, max-age=120, stale-while-revalidate=600",
            ))
            .json(d),
        Err(e) => e.error_response(),
    }
}

pub async fn detail_raw(db: web::Data<Handle>, path: web::Path<String>) -> HttpResponse {
    let db = db.into_inner();
    let uid = path.into_inner();
    let result = db_block(move || {
        let conn = db.conn().map_err(|e| ApiError::internal(e.to_string()))?;
        detail::raw_row(&conn, &uid)
    })
    .await;

    match result {
        Ok(Some(v)) => HttpResponse::Ok().json(v),
        Ok(None) => ApiError::NotFound("Аниме не найдено".into()).error_response(),
        Err(e) => e.error_response(),
    }
}

pub async fn suggest(
    db: web::Data<Handle>,
    q: web::Query<std::collections::HashMap<String, String>>,
) -> HttpResponse {
    let db = db.into_inner();
    let term = q.get("q").cloned().unwrap_or_default();
    let limit = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(8);
    let fts = db.fts_enabled();

    if term.trim().is_empty() {
        return HttpResponse::Ok().json(Vec::<Suggestion>::new());
    }

    let result = db_block(move || {
        let conn = db.conn().map_err(|e| ApiError::internal(e.to_string()))?;
        catalog::query_suggest(&conn, &term, fts, limit)
    })
    .await;

    match result {
        Ok(v) => HttpResponse::Ok()
            .insert_header(("cache-control", "public, max-age=60"))
            .json(v),
        Err(e) => e.error_response(),
    }
}

pub async fn filters(db: web::Data<Handle>) -> HttpResponse {
    let db = db.into_inner();
    let result = db_block(move || {
        let conn = db.conn().map_err(|e| ApiError::internal(e.to_string()))?;
        let (ymin, ymax) = detail::year_bounds(&conn)?;
        Ok(serde_json::json!({
            "formats": detail::distinct_values(&conn, "format")?,
            "statuses": detail::distinct_values(&conn, "status")?,
            "seasons": detail::distinct_values(&conn, "season")?,
            "countries": detail::distinct_values(&conn, "country_of_origin")?,
            "year_min": ymin,
            "year_max": ymax,
        }))
    })
    .await;

    match result {
        Ok(v) => HttpResponse::Ok()
            .insert_header(("cache-control", "public, max-age=300"))
            .json(v),
        Err(e) => e.error_response(),
    }
}

pub async fn genres(
    db: web::Data<Handle>,
    q: web::Query<std::collections::HashMap<String, String>>,
) -> HttpResponse {
    let db = db.into_inner();
    let category = q
        .get("category")
        .cloned()
        .filter(|c| c == "genre" || c == "tag" || c == "studio");
    let min_count = q.get("min_count").and_then(|v| v.parse().ok()).unwrap_or(1);

    let result = db_block(move || {
        let conn = db.conn().map_err(|e| ApiError::internal(e.to_string()))?;
        let rows = crate::loader::genres::list_genres(&conn, category.as_deref(), min_count)
            .map_err(ApiError::from)?;
        let genres: Vec<Genre> = rows
            .into_iter()
            .map(|g| {
                // Prefer the Russian label so the filter sheet is readable
                // without a client-side translation table.
                let name = g.name_ru.clone().unwrap_or_else(|| g.name_en.clone());
                Genre {
                    id: g.id,
                    slug: g.slug,
                    name,
                    name_ru: g.name_ru,
                    category: g.category,
                    count: Some(g.count),
                }
            })
            .collect();
        Ok(serde_json::json!({ "genres": genres }))
    })
    .await;

    match result {
        Ok(v) => HttpResponse::Ok()
            .insert_header(("cache-control", "public, max-age=300"))
            .json(v),
        Err(e) => e.error_response(),
    }
}

pub async fn genre_anime(
    db: web::Data<Handle>,
    path: web::Path<i64>,
    max_per_page: web::Data<i64>,
    q: web::Query<ListQuery>,
) -> HttpResponse {
    let db = db.into_inner();
    let genre_id = path.into_inner();
    let per_page_cap = *max_per_page.get_ref();
    let mut query = q.into_inner();
    let fts = db.fts_enabled();

    let result = db_block(move || {
        let conn = db.conn().map_err(|e| ApiError::internal(e.to_string()))?;
        let slug: String = conn
            .query_row("SELECT slug FROM genres WHERE id = ?1", [genre_id], |r| {
                r.get(0)
            })
            .map_err(|_| ApiError::NotFound("Жанр не найден".into()))?;
        // Reuse the shared filter builder instead of duplicating the join.
        query.genre = Some(slug);
        catalog::query_list(&conn, &query, fts, None, per_page_cap)
    })
    .await;

    match result {
        Ok(page) => HttpResponse::Ok().json(page),
        Err(e) => e.error_response(),
    }
}

// ==================================================================
// Accounts
// ==================================================================

pub async fn register(
    db: web::Data<Handle>,
    limiter: web::Data<Limiter>,
    req: HttpRequest,
    body: web::Json<RegisterBody>,
) -> HttpResponse {
    // 5 signups per 10 minutes per address: enough for a real person fixing a
    // typo, far too slow for a scripted mass signup.
    if let Err(r) = limited(&limiter, &req, 5).await {
        return r;
    }
    let db = db.into_inner();
    let body = body.into_inner();
    let result = db_block(move || auth::register(&db, body)).await;
    match result {
        Ok(r) => HttpResponse::Created()
            .insert_header(("cache-control", "no-store"))
            .json(r),
        Err(e) => e.error_response(),
    }
}

pub async fn login(
    db: web::Data<Handle>,
    limiter: web::Data<Limiter>,
    req: HttpRequest,
    body: web::Json<LoginBody>,
) -> HttpResponse {
    if let Err(r) = limited(&limiter, &req, 10).await {
        return r;
    }
    let db = db.into_inner();
    let body = body.into_inner();
    let result = db_block(move || auth::login(&db, body)).await;
    match result {
        Ok(r) => HttpResponse::Ok()
            .insert_header(("cache-control", "no-store"))
            .json(r),
        Err(e) => e.error_response(),
    }
}

pub async fn logout(db: web::Data<Handle>, req: HttpRequest) -> HttpResponse {
    let db = db.into_inner();
    let token = token_of(&req);
    let result = db_block(move || auth::logout(&db, token.as_deref())).await;
    match result {
        Ok(()) => HttpResponse::NoContent().finish(),
        Err(e) => e.error_response(),
    }
}

pub async fn me(db: web::Data<Handle>, req: HttpRequest) -> HttpResponse {
    let db = db.into_inner();
    let token = token_of(&req);
    let result = db_block(move || {
        let uid = authenticate(&db, token.as_deref())?;
        let conn = db.conn().map_err(|e| ApiError::internal(e.to_string()))?;
        auth::public_user(&conn, uid)
    })
    .await;

    match result {
        Ok(u) => HttpResponse::Ok()
            .insert_header(("cache-control", "private, no-store"))
            .json(u),
        Err(e) => e.error_response(),
    }
}

// ==================================================================
// Watchlist
// ==================================================================

pub async fn favorites_list(
    db: web::Data<Handle>,
    req: HttpRequest,
    filter: web::Query<favorites::ListFilter>,
) -> HttpResponse {
    let db = db.into_inner();
    let token = token_of(&req);
    let filter = filter.into_inner();
    let result = db_block(move || {
        let uid = authenticate(&db, token.as_deref())?;
        let conn = db.conn().map_err(|e| ApiError::internal(e.to_string()))?;
        favorites::list_with_anime(&conn, uid, &filter)
    })
    .await;

    match result {
        Ok(v) => HttpResponse::Ok()
            .insert_header(("cache-control", "private, no-store"))
            .json(v),
        Err(e) => e.error_response(),
    }
}

pub async fn favorites_counts(db: web::Data<Handle>, req: HttpRequest) -> HttpResponse {
    let db = db.into_inner();
    let token = token_of(&req);
    let result = db_block(move || {
        let uid = authenticate(&db, token.as_deref())?;
        let conn = db.conn().map_err(|e| ApiError::internal(e.to_string()))?;
        favorites::counts(&conn, uid)
    })
    .await;

    match result {
        Ok(v) => HttpResponse::Ok()
            .insert_header(("cache-control", "private, no-store"))
            .json(v),
        Err(e) => e.error_response(),
    }
}

pub async fn favorites_upsert(
    db: web::Data<Handle>,
    req: HttpRequest,
    body: web::Json<favorites::UpsertBody>,
) -> HttpResponse {
    let db = db.into_inner();
    let token = token_of(&req);
    let body = body.into_inner();
    let result = db_block(move || {
        let uid = authenticate(&db, token.as_deref())?;
        favorites::upsert(&db, uid, body)
    })
    .await;

    match result {
        Ok(v) => HttpResponse::Ok()
            .insert_header(("cache-control", "private, no-store"))
            .json(v),
        Err(e) => e.error_response(),
    }
}

pub async fn favorites_upsert_path(
    db: web::Data<Handle>,
    req: HttpRequest,
    path: web::Path<String>,
    body: web::Json<favorites::UpsertBody>,
) -> HttpResponse {
    let db = db.into_inner();
    let token = token_of(&req);
    let mut body = body.into_inner();
    // The path wins: a body carrying a different uid must not be honoured.
    body.uid = path.into_inner();
    let result = db_block(move || {
        let uid = authenticate(&db, token.as_deref())?;
        favorites::upsert(&db, uid, body)
    })
    .await;

    match result {
        Ok(v) => HttpResponse::Ok()
            .insert_header(("cache-control", "private, no-store"))
            .json(v),
        Err(e) => e.error_response(),
    }
}

pub async fn favorites_remove(
    db: web::Data<Handle>,
    req: HttpRequest,
    path: web::Path<String>,
) -> HttpResponse {
    let db = db.into_inner();
    let uid = path.into_inner();
    let token = token_of(&req);
    let result = db_block(move || {
        let user = authenticate(&db, token.as_deref())?;
        favorites::remove(&db, user, &uid)
    })
    .await;

    match result {
        Ok(()) => HttpResponse::NoContent().finish(),
        Err(e) => e.error_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::testing::test_db;
    use actix_web::http::StatusCode;
    use actix_web::test::TestRequest;

    fn req_with(headers: Vec<(&str, &str)>) -> HttpRequest {
        let mut b = TestRequest::default();
        for (k, v) in headers {
            b = b.insert_header((k, v));
        }
        b.to_http_request()
    }

    // ------------------------------------------------------ the token

    #[test]
    fn the_bearer_header_is_the_token() {
        assert_eq!(
            token_of(&req_with(vec![("authorization", "Bearer abc")])),
            Some("Bearer abc".into())
        );
    }

    #[test]
    fn a_request_without_the_header_has_no_token() {
        // Read-only endpoints stay usable anonymously, so a missing header is
        // `None` and not an error.
        assert_eq!(token_of(&req_with(vec![])), None);
    }

    #[test]
    fn the_scheme_is_not_stripped_here() {
        // `auth::authenticate` owns the prefix rules; this only moves the
        // string across the async boundary, and a token without "Bearer" has
        // to survive the trip so it can be rejected there.
        let r = req_with(vec![("authorization", "Basic abc")]);
        assert_eq!(token_of(&r), Some("Basic abc".into()));
    }

    // -------------------------------------------------- the gatekeeper

    #[test]
    fn a_missing_token_is_unauthorised_rather_than_anonymous() {
        // The other half of the rule above: where a session is required, no
        // token is a 401 and never a silent success.
        let db = test_db();
        let e = authenticate(&db.handle, None).unwrap_err();
        assert_eq!(e.status_code(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn a_garbage_token_is_unauthorised() {
        let db = test_db();
        let e = authenticate(&db.handle, Some("Bearer nope")).unwrap_err();
        assert_eq!(e.status_code(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn a_real_session_gets_through_the_gate() {
        let db = test_db();
        let r = crate::api::auth::register(
            &db.handle,
            RegisterBody {
                username: "user".into(),
                email: None,
                password: "password123".into(),
            },
        )
        .expect("register");
        let uid = authenticate(&db.handle, Some(&format!("Bearer {}", r.token))).expect("uid");
        assert_eq!(uid, r.user.id);
    }

    // ----------------------------------------------------- the limiter

    #[tokio::test]
    async fn the_limiter_allows_up_to_the_budget() {
        let limiter = Limiter::new(600);
        let r = req_with(vec![]);
        for i in 0..5 {
            assert!(limited(&limiter, &r, 5).await.is_ok(), "запрос {}", i);
        }
    }

    #[tokio::test]
    async fn the_limiter_answers_429_with_a_retry_after() {
        // The rate limiter is the only thing between a script and a
        // credential-stuffing run against /api/auth/login, so exceeding the
        // budget has to be visible to the client as a retry, not a generic
        // error it cannot act on.
        let limiter = Limiter::new(600);
        let r = req_with(vec![]);
        for _ in 0..3 {
            limited(&limiter, &r, 3).await.expect("первые три проходят");
        }
        let res = limited(&limiter, &r, 3)
            .await
            .expect_err("четвёртый отклонён");
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(res.headers().contains_key("retry-after"), "нет retry-after");
        assert!(res.headers().contains_key("content-type"), "ответ не JSON");
    }

    #[tokio::test]
    async fn each_client_gets_its_own_budget() {
        // One shared bucket keyed on nothing would lock out a whole office
        // behind one noisy machine.
        let limiter = Limiter::new(600);
        let a = req_with(vec![("x-forwarded-for", "203.0.113.1")]);
        let b = req_with(vec![("x-forwarded-for", "203.0.113.2")]);
        limited(&limiter, &a, 2).await.expect("a");
        limited(&limiter, &a, 2).await.expect("a");
        limited(&limiter, &a, 2).await.expect_err("a исчерпал");
        assert!(
            limited(&limiter, &b, 2).await.is_ok(),
            "b затронут чужой бюджетом"
        );
    }
}
