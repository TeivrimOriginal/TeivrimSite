#![recursion_limit = "512"]

mod api;
mod db;
mod genres;
mod loader;
mod sources;
mod translate;

use actix_web::{web, App, HttpServer};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!("=== Anime Loader (RU-only mode) ===");

    {
        let conn = db::open().expect("DB open");
        db::ensure_schema(&conn).expect("Schema");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM anime", [], |r| r.get(0))
            .unwrap_or(0);
        let ru_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM anime WHERE title_russian IS NOT NULL AND title_russian != ''",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        println!("[DB] записей на старте: {} (с русским: {})", count, ru_count);
    }

    // AniList — базовые данные (английский + метаданные)
    tokio::spawn(async {
        if let Err(e) = loader::load_anilist().await {
            eprintln!("[AniList ERR] {}", e);
        }
        println!("\n=== AniList ЗАВЕРШЁН ===");
    });

    // Shikimori — русские названия (параллельно)
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        if let Err(e) = loader::load_shikimori().await {
            eprintln!("[Shikimori ERR] {}", e);
        }
        println!("\n=== Shikimori ЗАВЕРШЁН ===");
    });

    // Сопоставление жанров — после загрузки
    tokio::spawn(async {
        tokio::time::sleep(std::time::Duration::from_secs(120)).await;
        println!("[WORKER] Сопоставление жанров...");
        if let Err(e) = genres::match_all_genres().await {
            eprintln!("[Genres ERR] {}", e);
        }
        println!("[WORKER] Жанры готовы");
    });

    // ❌ Переводчик Google НЕ запускается
    // ❌ Jikan НЕ запускается

    println!("\n=== СЕРВЕР: http://127.0.0.1:8082 ===\n");

    HttpServer::new(|| {
        App::new()
            .route("/", web::get().to(api::index_html))
            .route("/detail", web::get().to(api::detail_html))
            .route("/api/list", web::get().to(api::api_list))
            .route("/api/count", web::get().to(api::api_count))
            .route("/api/filters", web::get().to(api::api_filters))
            .route("/api/progress", web::get().to(api::api_progress))
            .route("/api/genres", web::get().to(api::api_genres))
            .route("/api/genres/{id}/anime", web::get().to(api::api_genre_anime))
            .route("/api/anime/{id}", web::get().to(api::api_detail))
    })
    .bind(("127.0.0.1", 8082))?
    .run()
    .await
}