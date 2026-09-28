pub mod auth;
pub mod catalog;
pub mod detail;
pub mod favorites;
pub mod handlers;
pub mod images;
pub mod stats;
pub mod sync;

use actix_web::http::Method;
use actix_web::{web, HttpRequest, HttpResponse};

pub fn configure(cfg: &mut web::ServiceConfig, max_per_page: i64, cors_origins: &[String]) {
    let cors = crate::http::cors::Cors::new(cors_origins.to_vec());

    cfg.service(
        web::scope("/api")
            .app_data(web::Data::new(max_per_page))
            // --- catalogue ---
            .route("/anime", web::get().to(handlers::list))
            .route("/anime/{uid}", web::get().to(handlers::detail))
            .route("/anime/{uid}/raw", web::get().to(handlers::detail_raw))
            .route("/search/suggest", web::get().to(handlers::suggest))
            .route("/filters", web::get().to(handlers::filters))
            .route("/genres", web::get().to(handlers::genres))
            .route("/genres/{id}/anime", web::get().to(handlers::genre_anime))
            .route("/stats", web::get().to(stats::stats))
            // --- catalogue sync ---
            .route("/sync/status", web::get().to(sync::status))
            .route("/sync/start", web::post().to(sync::start))
            .route("/sync/abort", web::post().to(sync::abort))
            // --- accounts ---
            .route("/auth/register", web::post().to(handlers::register))
            .route("/auth/login", web::post().to(handlers::login))
            .route("/auth/logout", web::post().to(handlers::logout))
            .route("/auth/me", web::get().to(handlers::me))
            // --- watchlist ---
            .route("/favorites", web::get().to(handlers::favorites_list))
            .route("/favorites", web::post().to(handlers::favorites_upsert))
            .route("/favorites/counts", web::get().to(handlers::favorites_counts))
            .route("/favorites/{uid}", web::patch().to(handlers::favorites_upsert_path))
            .route("/favorites/{uid}", web::delete().to(handlers::favorites_remove))
            // --- images ---
            .route("/img", web::get().to(images::proxy))
            // Preflight catch-all, registered last so it only sees OPTIONS
            // requests that no real route claimed.
            .route(
                "/{tail:.*}",
                web::route()
                    .method(Method::OPTIONS)
                    .to(move |req: HttpRequest| preflight(cors.clone(), req)),
            ),
    );
}

/// Answers a CORS preflight. Kept as a real route rather than middleware so the
/// response body type stays the same as every other handler.
async fn preflight(cors: crate::http::cors::Cors, req: HttpRequest) -> HttpResponse {
    let origin = req
        .headers()
        .get(actix_web::http::header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let mut headers = actix_web::http::header::HeaderMap::new();
    cors.apply_headers(&mut headers, origin.as_deref());

    // HttpResponseBuilder has no headers_mut(), so the policy is computed into
    // a map first and then replayed onto the builder.
    let mut builder = HttpResponse::NoContent();
    for (name, value) in headers.iter() {
        builder.insert_header((name.clone(), value.clone()));
    }
    builder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::testing::test_db;
    use actix_web::body::to_bytes;
    use actix_web::dev::ServiceResponse;
    use actix_web::http::StatusCode;
    use actix_web::test;
    use actix_web::App;
    use serde_json::Value;

    /// The same wiring `main` does, minus the loaders: a route that resolves
    /// here is a route that exists in the real server.
    ///
    /// A macro rather than a function because `init_service` hands back
    /// `impl Service<Request, ...>` and `actix_http` is not a direct dependency,
    /// so the return type cannot be written out.
    ///
    /// The second form adds app data, for the few tests that need the loaders
    /// to be present; the default form deliberately has no loader context,
    /// which is the `LOADERS_ON_START=0` deployment.
    macro_rules! app {
        ($db:expr) => { app!($db, ) };
        ($db:expr, $($extra:tt)*) => {
            test::init_service(
                App::new()
                    .app_data(web::Data::new($db.handle.clone()))
                    .app_data(web::Data::new(Some(String::from("admin-token"))))
                    .app_data(web::Data::new(crate::api::images::ImageCache::default()))
                    .app_data(web::Data::new(crate::http::ratelimit::Limiter::new(600)))
                    .app_data(web::Data::new(crate::http::ratelimit::Limiter::new(60)))
                    $($extra)*
                    .configure(|cfg| configure(cfg, 100, &["*".to_string()])),
            )
            .await
        };
    }

    async fn body_json<B>(res: ServiceResponse<B>) -> Value
    where
        B: actix_web::body::MessageBody,
        B::Error: std::fmt::Display,
    {
        let bytes = match to_bytes(res.into_body()).await {
            Ok(b) => b,
            Err(e) => panic!("не удалось прочитать тело ответа: {}", e),
        };
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    }

    fn seeded(db: &crate::db::testing::TestDb) {
        let c = db.conn();
        c.execute(
            "INSERT INTO anime (uid, anilist_id, title_romaji, title_key, title_english,
                title_russian, start_year, score, format, is_adult, episodes, popularity,
                created_at, updated_at)
             VALUES ('al:16498', 16498, 'Shingeki no Kyojin', 'shingeki no kyojin', 'Attack on Titan',
                'Атака Титанов', 2013, 84, 'TV', 0, 25, 100, 1, 1)",
            [],
        )
        .unwrap();
        // The search endpoints read the FTS index, which a real sync fills in
        // after every page; a fixture that skipped it would test the empty case.
        crate::db::rebuild_fts(&c).unwrap();
    }

    #[actix_web::test]
    async fn the_catalogue_list_answers() {
        let db = test_db();
        seeded(&db);
        let app = app!(&db);
        let res =
            test::call_service(&app, test::TestRequest::get().uri("/api/anime").to_request()).await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers().get("x-total-count").unwrap(), "1");
        let v = body_json(res).await;
        assert_eq!(v["total"], 1);
        assert_eq!(v["items"][0]["uid"], "al:16498");
    }

    #[actix_web::test]
    async fn the_detail_endpoint_answers_and_404s() {
        let db = test_db();
        seeded(&db);
        let app = app!(&db);

        let res = test::call_service(
            &app,
            test::TestRequest::get().uri("/api/anime/al:16498").to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let v = body_json(res).await;
        assert_eq!(v["uid"], "al:16498");
        assert_eq!(v["title_russian"], "Атака Титанов");
        assert_eq!(v["ids"]["anilist"], 16498);

        let res = test::call_service(
            &app,
            test::TestRequest::get().uri("/api/anime/al:99999").to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let v = body_json(res).await;
        assert_eq!(v["error"]["code"], "not_found");
    }

    #[actix_web::test]
    async fn the_raw_dump_answers() {
        let db = test_db();
        seeded(&db);
        let app = app!(&db);
        let res = test::call_service(
            &app,
            test::TestRequest::get().uri("/api/anime/al:16498/raw").to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let v = body_json(res).await;
        assert_eq!(v["uid"], "al:16498");
        assert!(v.get("title_key").is_some());
    }

    #[actix_web::test]
    async fn an_empty_suggestion_term_is_an_empty_list() {
        let db = test_db();
        let app = app!(&db);
        let res = test::call_service(
            &app,
            test::TestRequest::get().uri("/api/search/suggest?q=").to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let v = body_json(res).await;
        assert_eq!(v.as_array().unwrap().len(), 0);
    }

    #[actix_web::test]
    async fn the_suggestion_endpoint_answers() {
        let db = test_db();
        seeded(&db);
        let app = app!(&db);
        let res = test::call_service(
            &app,
            test::TestRequest::get().uri("/api/search/suggest?q=shingeki").to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let v = body_json(res).await;
        assert_eq!(v[0]["uid"], "al:16498");
    }

    #[actix_web::test]
    async fn the_filters_endpoint_answers() {
        let db = test_db();
        seeded(&db);
        let app = app!(&db);
        let res = test::call_service(&app, test::TestRequest::get().uri("/api/filters").to_request()).await;
        assert_eq!(res.status(), StatusCode::OK);
        let v = body_json(res).await;
        assert_eq!(v["formats"][0], "TV");
        assert_eq!(v["year_min"], 2013);
        assert_eq!(v["year_max"], 2013);
    }

    #[actix_web::test]
    async fn the_genres_endpoint_answers() {
        let db = test_db();
        let app = app!(&db);
        let res = test::call_service(&app, test::TestRequest::get().uri("/api/genres").to_request()).await;
        assert_eq!(res.status(), StatusCode::OK);
        let v = body_json(res).await;
        assert!(v["genres"].as_array().is_some());
    }

    #[actix_web::test]
    async fn an_unknown_genre_id_is_a_404() {
        let db = test_db();
        let app = app!(&db);
        let res = test::call_service(
            &app,
            test::TestRequest::get().uri("/api/genres/404/anime").to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn the_stats_endpoint_answers() {
        let db = test_db();
        seeded(&db);
        let app = app!(&db);
        let res = test::call_service(&app, test::TestRequest::get().uri("/api/stats").to_request()).await;
        assert_eq!(res.status(), StatusCode::OK);
        let v = body_json(res).await;
        assert_eq!(v["anime"], 1);
        assert_eq!(v["with_russian"], 1);
        assert_eq!(v["with_anilist"], 1);
        assert_eq!(v["year_min"], 2013);
    }

    #[actix_web::test]
    async fn the_sync_status_is_public() {
        // A deployment dashboard watches a long import without credentials.
        let _serial = crate::loader::RUN_FLAG_LOCK.lock().await;
        let db = test_db();
        let app = app!(&db);
        let res = test::call_service(
            &app,
            test::TestRequest::get().uri("/api/sync/status").to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let v = body_json(res).await;
        assert_eq!(v["running"], false);
        assert_eq!(v["anime"], 0);
        // No loaders are wired into this test app, so the schedule is off and
        // says why rather than reporting an interval nobody is honouring.
        assert_eq!(v["schedule"]["enabled"], false);
    }

    #[actix_web::test]
    async fn the_sync_status_says_so_when_a_pass_is_in_flight() {
        let _serial = crate::loader::RUN_FLAG_LOCK.lock().await;
        let db = test_db();
        let app = app!(&db);
        let _claimed = crate::loader::try_begin_run().expect("флаг синхронизации");
        let res = test::call_service(
            &app,
            test::TestRequest::get().uri("/api/sync/status").to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(body_json(res).await["running"], true);
    }

    #[actix_web::test]
    async fn the_sync_status_reports_the_refresh_schedule_when_there_is_one() {
        // "running" on its own is how a dashboard ends up lying: the operator
        // needs to know there will be another pass without pressing anything.
        let db = test_db();
        let ctx = crate::db::testing::test_ctx();
        let app = app!(&db, .app_data(web::Data::new(std::sync::Arc::new(ctx))));
        let res = test::call_service(
            &app,
            test::TestRequest::get().uri("/api/sync/status").to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let v = body_json(res).await;
        assert_eq!(v["schedule"]["enabled"], true, "ответ: {}", v);
        assert!(
            v["schedule"]["interval_secs"].as_u64().unwrap_or(0) > 0,
            "интервал не показан: {}",
            v
        );
    }

    #[actix_web::test]
    async fn starting_a_sync_while_one_is_running_is_refused_rather_than_queued() {
        // Two passes over the same checkpoints would double the request rate
        // against APIs that answer 429, so the second one is told no.
        let _serial = crate::loader::RUN_FLAG_LOCK.lock().await;
        let db = test_db();
        let ctx = crate::db::testing::test_ctx();
        let app = app!(&db, .app_data(web::Data::new(std::sync::Arc::new(ctx))));
        let _claimed = crate::loader::try_begin_run().expect("флаг синхронизации");
        let res = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/sync/start")
                .insert_header(("x-admin-token", "admin-token"))
                .to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let v = body_json(res).await;
        assert_eq!(v["started"], false, "ответ: {}", v);
        assert_eq!(v["reason"], "уже выполняется");
    }

    #[actix_web::test]
    async fn starting_a_sync_without_the_admin_token_is_forbidden() {
        // With no token presented the endpoints must not fall open, or anyone
        // who finds the route can start a multi-hour import.
        let db = test_db();
        let app = app!(&db);
        for uri in ["/api/sync/start", "/api/sync/abort"] {
            let res = test::call_service(&app, test::TestRequest::post().uri(uri).to_request()).await;
            assert_eq!(res.status(), StatusCode::FORBIDDEN, "{}", uri);
        }
    }

    #[actix_web::test]
    async fn the_watchlist_endpoints_need_a_session() {
        let db = test_db();
        seeded(&db);
        let app = app!(&db);
        for uri in ["/api/favorites", "/api/favorites/counts"] {
            let res = test::call_service(&app, test::TestRequest::get().uri(uri).to_request()).await;
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{}", uri);
        }
    }

    #[actix_web::test]
    async fn a_full_signup_login_and_watchlist_round_trip() {
        let db = test_db();
        seeded(&db);
        let app = app!(&db);

        let res = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/auth/register")
                .set_json(serde_json::json!({ "username": "user", "password": "password123" }))
                .to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CREATED);
        let v = body_json(res).await;
        let token = v["token"].as_str().unwrap().to_string();

        let res = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/auth/me")
                .insert_header(("authorization", format!("Bearer {}", token)))
                .to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let v = body_json(res).await;
        assert_eq!(v["username"], "user");

        let res = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/favorites")
                .insert_header(("authorization", format!("Bearer {}", token)))
                .set_json(serde_json::json!({ "uid": "al:16498", "status": "watching", "score": 9 }))
                .to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let v = body_json(res).await;
        assert_eq!(v["status"], "watching");
        assert_eq!(v["episodes"], 25);

        let res = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/favorites")
                .insert_header(("authorization", format!("Bearer {}", token)))
                .to_request(),
        )
        .await;
        let v = body_json(res).await;
        assert_eq!(v[0]["uid"], "al:16498", "запись приклеена к каталогу");

        // The catalogue filter uses the same session.
        let res = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/anime?in_list=watching")
                .insert_header(("authorization", format!("Bearer {}", token)))
                .to_request(),
        )
        .await;
        let v = body_json(res).await;
        assert_eq!(v["total"], 1);

        let res = test::call_service(
            &app,
            test::TestRequest::delete()
                .uri("/api/favorites/al:16498")
                .insert_header(("authorization", format!("Bearer {}", token)))
                .to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        let res = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/auth/logout")
                .insert_header(("authorization", format!("Bearer {}", token)))
                .to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);

        // The token is dead after the logout, so the session is gone.
        let res = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/auth/me")
                .insert_header(("authorization", format!("Bearer {}", token)))
                .to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn a_rejected_signup_answers_400_with_a_readable_message() {
        let db = test_db();
        let app = app!(&db);
        let res = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/auth/register")
                .set_json(serde_json::json!({ "username": "u", "password": "short" }))
                .to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let v = body_json(res).await;
        assert_eq!(v["error"]["code"], "bad_request");
        assert!(!v["error"]["message"].as_str().unwrap().is_empty());
    }

    #[actix_web::test]
    async fn a_patch_to_the_watchlist_takes_the_uid_from_the_path() {
        // A body carrying a different uid must not be honoured: the path is the
        // thing the client was told to act on.
        let db = test_db();
        seeded(&db);
        let app = app!(&db);
        let res = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/auth/register")
                .set_json(serde_json::json!({ "username": "user", "password": "password123" }))
                .to_request(),
        )
        .await;
        let v = body_json(res).await;
        let token = v["token"].as_str().unwrap().to_string();

        let res = test::call_service(
            &app,
            test::TestRequest::patch()
                .uri("/api/favorites/al:16498")
                .insert_header(("authorization", format!("Bearer {}", token)))
                .set_json(serde_json::json!({ "uid": "al:other", "status": "planned" }))
                .to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);
        let v = body_json(res).await;
        assert_eq!(v["uid"], "al:16498");
        assert_eq!(v["status"], "planned");
    }

    #[actix_web::test]
    async fn the_image_proxy_refuses_a_foreign_host() {
        let db = test_db();
        let app = app!(&db);
        let res = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/img?u=https%3A%2F%2Fevil.example%2Fx.jpg")
                .to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[actix_web::test]
    async fn the_image_proxy_needs_a_url() {
        let db = test_db();
        let app = app!(&db);
        let res = test::call_service(&app, test::TestRequest::get().uri("/api/img").to_request()).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[actix_web::test]
    async fn a_preflight_is_answered_with_the_cors_policy() {
        let db = test_db();
        let app = app!(&db);
        let res = test::call_service(
            &app,
            test::TestRequest::default()
                .method(actix_web::http::Method::OPTIONS)
                .uri("/api/anime")
                .insert_header(("origin", "https://app.example"))
                .to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::NO_CONTENT);
        assert_eq!(res.headers().get("access-control-allow-origin").unwrap(), "*");
    }

    #[actix_web::test]
    async fn an_unknown_api_path_is_a_404() {
        // A mistyped endpoint must not come back as 200 or as an HTML page the
        // client cannot parse. (The JSON body of the 404 comes from the
        // app-level default service in `main`, which is not part of this scope.)
        let db = test_db();
        let app = app!(&db);
        let res = test::call_service(&app, test::TestRequest::get().uri("/api/nope").to_request()).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn a_bad_query_parameter_is_rejected_instead_of_ignored() {
        let db = test_db();
        let app = app!(&db);
        let res = test::call_service(
            &app,
            test::TestRequest::get().uri("/api/anime?page=second").to_request(),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
}
