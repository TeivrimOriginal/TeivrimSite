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
