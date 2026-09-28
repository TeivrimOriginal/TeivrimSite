//! Anime catalogue server.
//!
//! Aggregates AniList, Kitsu and Shikimori into one SQLite database, and serves
//! a JSON API plus a static web frontend from the same origin so there is no
//! cross-origin deployment to configure.

#![recursion_limit = "512"]

mod api;
mod config;
mod db;
mod error;
#[cfg(test)]
mod guard;
mod http;
mod loader;
mod models;
mod sources;
mod upstream;

use actix_web::middleware::Compress;
use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer};
use config::Config;
use error::{log_error, log_info, log_warn};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // A missing .env is normal in production, so its absence is not an error.
    let _ = dotenvy::dotenv();
    let cfg = Arc::new(Config::from_env());

    log_info("=== Anime DB 2.0 ===");

    let db = match db::Db::open(&cfg.db_path, cfg.pool_size) {
        Ok(db) => db,
        Err(e) => {
            log_error(&format!("[db] {}", e));
            std::process::exit(1);
        }
    };

    {
        let conn = match db.conn() {
            Ok(c) => c,
            Err(e) => {
                log_error(&format!("[db] {}", e));
                std::process::exit(1);
            }
        };
        // A crash mid-import would otherwise leave tasks looking like they are
        // still progressing.
        api::sync::reconcile_on_boot(&conn);
    }

    let sources = match sources::Sources::new(&cfg) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            log_error(&format!("[источники] {}", e));
            std::process::exit(1);
        }
    };

    if !cfg.frontend_dir.exists() {
        log_warn(&format!(
            "[frontend] каталог {} не найден — интерфейс не будет отдаваться",
            cfg.frontend_dir.display()
        ));
    }

    let admin_token = std::env::var("SYNC_ADMIN_TOKEN").ok().filter(|t| !t.is_empty());
    if admin_token.is_none() {
        log_warn("[sync] SYNC_ADMIN_TOKEN не задан — /api/sync/start и /api/sync/abort будут отклонять все запросы");
    }

    let ctx = Arc::new(loader::Ctx {
        db: db.clone(),
        sources,
        cfg: cfg.clone(),
    });

    // An API-only deployment (a second container behind a read-only volume,
    // say) sets LOADERS_ON_START=0 and lets a dedicated importer do the work.
    if cfg.loaders_on_start {
        let bg = ctx.clone();
        tokio::spawn(async move {
            loader::run_all((*bg).clone()).await;
        });
    } else {
        log_info("[sync] загрузчики отключены (LOADERS_ON_START=0)");
    }

    // Housekeeping: drop expired sessions hourly.
    {
        let bg = db.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                let d = bg.clone();
                let _ = web::block(move || {
                    if let Ok(conn) = d.conn() {
                        api::auth::purge_expired(&conn);
                    }
                })
                .await;
            }
        });
    }

    let total: i64 = db
        .with(|c| c.query_row("SELECT COUNT(*) FROM anime", [], |r| r.get(0)))
        .unwrap_or(0);
    log_info(&format!(
        "[старт] слушаю {} | записей в каталоге: {}",
        cfg.socket_addr(),
        total
    ));
    log_info(&format!("[старт] CORS: {}", cfg.cors_origins.join(", ")));

    let db_data = web::Data::new(db);
    let token_data = web::Data::new(admin_token);
    let ctx_data = web::Data::new(ctx);
    let image_cache = web::Data::new(api::images::ImageCache::default());
    let frontend_dir = cfg.frontend_dir.clone();
    let max_per_page = cfg.max_per_page;
    let cors = http::cors::Cors::new(cfg.cors_origins.clone());
    let cors_origins = cfg.cors_origins.clone();
    // Shared between the auth endpoints (10 per 10 min) and the image proxy
    // (240 per minute); the per-endpoint budget is passed at the call site.
    let auth_limiter = web::Data::new(http::ratelimit::Limiter::new(600));
    let img_limiter = web::Data::new(http::ratelimit::Limiter::new(60));

    HttpServer::new(move || {
        App::new()
            .app_data(db_data.clone())
            .app_data(token_data.clone())
            .app_data(ctx_data.clone())
            .app_data(image_cache.clone())
            .app_data(auth_limiter.clone())
            .app_data(img_limiter.clone())
            .app_data(web::Data::new(max_per_page))
            // Outermost so it wraps everything, including the static routes.
            .wrap(cors.clone())
            .wrap(Compress::default())
            .wrap(http::Decorate)
            .configure(api_routes(
                frontend_dir.clone(),
                max_per_page,
                cors_origins.clone(),
            ))
    })
    .client_request_timeout(Duration::from_secs(30))
    .keep_alive(Duration::from_secs(75))
    .workers(num_workers())
    .bind(cfg.socket_addr())?
    .run()
    .await
}

fn api_routes(root: PathBuf, max_per_page: i64, cors_origins: Vec<String>) -> impl FnOnce(&mut web::ServiceConfig) {
    move |cfg: &mut web::ServiceConfig| {
        api::configure(cfg, max_per_page, &cors_origins);
        pages(cfg, root.clone());
    }
}

/// The frontend entry points.
///
/// Routes are explicit instead of a catch-all so a mistyped API path returns a
/// JSON 404 rather than the catalogue page. Assets live under `/static/`.
fn pages(cfg: &mut web::ServiceConfig, root: PathBuf) {
    cfg.route(
        "/",
        web::get().to({
            let root = root.clone();
            move |req: HttpRequest| {
                let root = root.clone();
                async move { http::static_files::serve(&req, &root, "index.html").await }
            }
        }),
    );

    // Client-side route: /anime/al:16498
    cfg.route(
        "/anime/{uid}",
        web::get().to({
            let root = root.clone();
            move |req: HttpRequest, _uid: web::Path<String>| {
                let root = root.clone();
                async move {
                    let res = http::static_files::serve(&req, &root, "anime.html").await;
                    if res.status().is_success() {
                        res
                    } else {
                        // The SPA shell still renders its own "not found" state
                        // from the URL, so serve index.html rather than a 404
                        // that the client cannot render.
                        http::static_files::serve(&req, &root, "index.html").await
                    }
                }
            }
        }),
    );

    cfg.service(
        web::resource("/static/{path:.*}").route(web::get().to({
            let root = root.clone();
            move |req: HttpRequest, path: web::Path<String>| {
                let root = root.clone();
                async move {
                    // The capture is the tail only, so the directory has to be
                    // put back or every asset resolves one level too high.
                    let rel = format!("static/{}", path.into_inner());
                    http::static_files::serve(&req, &root, &rel).await
                }
            }
        })),
    );

    // Well-known files that browsers and crawlers request from the site root
    // rather than from /static/.
    for (path, file) in [
        ("/robots.txt", "robots.txt"),
        ("/manifest.webmanifest", "manifest.webmanifest"),
        ("/sitemap.xml", "sitemap.xml"),
        ("/favicon.ico", "static/icon-192.png"),
    ] {
        cfg.route(
            path,
            web::get().to({
                let root = root.clone();
                let file = file.to_string();
                move |req: HttpRequest| {
                    let root = root.clone();
                    let file = file.clone();
                    async move { http::static_files::serve(&req, &root, &file).await }
                }
            }),
        );
    }

    // Health probe for a container orchestrator.
    cfg.route(
        "/healthz",
        web::get().to(|| async { HttpResponse::Ok().json(serde_json::json!({ "ok": true })) }),
    );

    cfg.default_service(web::to(|| async {
        HttpResponse::NotFound().content_type("application/json").json(serde_json::json!({
            "error": { "code": "not_found", "message": "Маршрут не найден" }
        }))
    }));
}

fn num_workers() -> usize {
    std::env::var("WORKERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get().clamp(2, 8))
                .unwrap_or(4)
        })
}
