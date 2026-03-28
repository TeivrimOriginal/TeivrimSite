mod models;
mod db;
mod handlers;
mod templates;

use axum::{
    routing::{post, get},
    Router,
    extract::{Extension, Path},
    response::Html,
};
use tower_http::services::ServeDir;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::fs;

#[tokio::main]
async fn main() {
    let pool = db::init_database().await;
    
    let current_dir = std::env::current_dir().expect("Не удалось получить текущую директорию");
    let uploads_dir = current_dir.join("uploads").join("avatars");
    
    if !uploads_dir.exists() {
        fs::create_dir_all(&uploads_dir).await.unwrap_or_default();
        println!("✅ Директория создана: {}", uploads_dir.display());
    }
    
    let state = Arc::new(db::AppState {
        db: pool,
        sessions: tokio::sync::Mutex::new(std::collections::HashMap::new()),
    });
    
    let app = Router::new()
        .route("/", get(home_page))
        .route("/login-page", get(login_page))
        .route("/register-page", get(register_page))
        .route("/profile/:username", get(profile_page))
        .route("/register", post(handlers::register))
        .route("/login", post(handlers::login))
        .route("/logout", post(handlers::logout))
        .route("/api/profile/:username", get(handlers::get_profile))
        .route("/api/profile/update", post(handlers::update_profile))
        .route("/api/profile/avatar", post(handlers::upload_avatar))
        .route("/api/search", get(handlers::search_users))
        .route("/api/chats", get(handlers::get_chats))
        .route("/message", post(handlers::send_message))
        .route("/messages/:user1/:user2", get(handlers::get_messages))
        .nest_service("/uploads", ServeDir::new("uploads"))
        .layer(Extension(state));
    
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("\n🚀 Сервер запущен: http://{}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn home_page() -> Html<String> {
    templates::home_page()
}

async fn login_page() -> Html<String> {
    templates::login_page()
}

async fn register_page() -> Html<String> {
    templates::register_page()
}

async fn profile_page(Path(username): Path<String>) -> Html<String> {
    templates::profile_page(&username)
}