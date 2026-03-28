use axum::{
    Json, extract::{Extension, Multipart, Path, Query},
    http::StatusCode,
};
use std::sync::Arc;
use tokio::fs;
use uuid::Uuid;
use crate::db::AppState;
use crate::models::{UserReg, UserLogin, Response};

pub async fn register(
    Extension(state): Extension<Arc<AppState>>,
    Json(user): Json<UserReg>,
) -> (StatusCode, Json<Response>) {
    match state.create_user(&user).await {
        Ok(_) => (
            StatusCode::CREATED,
            Json(Response {
                success: true,
                message: format!("Пользователь {} создан", user.username),
            }),
        ),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(Response {
                success: false,
                message: format!("Ошибка: {}", e),
            }),
        ),
    }
}

pub async fn login(
    Extension(state): Extension<Arc<AppState>>,
    Json(login): Json<UserLogin>,
) -> (StatusCode, Json<Response>) {
    match state.find_user(&login.username, &login.password).await {
        Ok(Some(_user_id)) => {
            (
                StatusCode::OK,
                Json(Response {
                    success: true,
                    message: format!("Добро пожаловать, {}!", login.username),
                }),
            )
        }
        Ok(None) => (
            StatusCode::UNAUTHORIZED,
            Json(Response {
                success: false,
                message: "Неверное имя пользователя или пароль".to_string(),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Response {
                success: false,
                message: format!("Ошибка: {}", e),
            }),
        ),
    }
}

pub async fn logout(
    Extension(state): Extension<Arc<AppState>>,
) -> (StatusCode, Json<Response>) {
    let mut sessions = state.sessions.lock().await;
    sessions.clear();
    
    (
        StatusCode::OK,
        Json(Response {
            success: true,
            message: "Вы вышли из системы".to_string(),
        }),
    )
}

pub async fn get_profile(
    Extension(state): Extension<Arc<AppState>>,
    Path(username): Path<String>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> (StatusCode, Json<serde_json::Value>) {
    let current_user = params.get("current").map(|s| s.as_str()).unwrap_or("");
    
    match state.get_profile(&username, current_user).await {
        Ok(Some(profile)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "profile": profile,
                "can_edit": profile.username == current_user
            })),
        ),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "success": false,
                "error": "Пользователь не найден"
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Ошибка: {}", e)
            })),
        ),
    }
}

pub async fn search_users(
    Extension(state): Extension<Arc<AppState>>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> (StatusCode, Json<serde_json::Value>) {
    let query = params.get("q").map(|s| s.as_str()).unwrap_or("");
    let current_user = params.get("current").map(|s| s.as_str()).unwrap_or("");
    
    if query.is_empty() {
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "users": []
            })),
        );
    }
    
    match state.search_users(query, current_user).await {
        Ok(users) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "users": users
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Ошибка: {}", e)
            })),
        ),
    }
}

pub async fn update_profile(
    Extension(state): Extension<Arc<AppState>>,
    Json(data): Json<serde_json::Value>,
) -> (StatusCode, Json<Response>) {
    let username = data.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let birth_date = data.get("birth_date").and_then(|v| v.as_str()).unwrap_or("");
    let email = data.get("email").and_then(|v| v.as_str()).unwrap_or("");
    let description = data.get("description").and_then(|v| v.as_str()).unwrap_or("");
    let current_user = data.get("current_user").and_then(|v| v.as_str()).unwrap_or("");
    
    if username.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(Response {
                success: false,
                message: "Username обязателен".to_string(),
            }),
        );
    }
    
    match state.update_profile(username, birth_date, email, description, current_user).await {
        Ok(_) => (
            StatusCode::OK,
            Json(Response {
                success: true,
                message: "Профиль обновлен".to_string(),
            }),
        ),
        Err(e) => (
            StatusCode::FORBIDDEN,
            Json(Response {
                success: false,
                message: format!("Ошибка: {}", e),
            }),
        ),
    }
}

pub async fn upload_avatar(
    Extension(state): Extension<Arc<AppState>>,
    mut multipart: Multipart,
) -> (StatusCode, Json<Response>) {
    let mut username = String::new();
    let mut current_user = String::new();
    let mut avatar_data: Option<Vec<u8>> = None;
    
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = match field.name() {
            Some(n) => n.to_string(),
            None => continue,
        };
        
        if name == "username" {
            if let Ok(text) = field.text().await {
                username = text;
            }
        } else if name == "current_user" {
            if let Ok(text) = field.text().await {
                current_user = text;
            }
        } else if name == "avatar" {
            if let Ok(data) = field.bytes().await {
                avatar_data = Some(data.to_vec());
            }
        }
    }
    
    if username.is_empty() || avatar_data.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(Response {
                success: false,
                message: "Необходимы username и файл аватарки".to_string(),
            }),
        );
    }
    
    let extension = "jpg";
    let unique_filename = format!("{}_{}.{}", username, Uuid::new_v4(), extension);
    let file_path = format!("uploads/avatars/{}", unique_filename);
    
    if let Err(e) = fs::write(&file_path, avatar_data.unwrap()).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Response {
                success: false,
                message: format!("Ошибка сохранения файла: {}", e),
            }),
        );
    }
    
    let web_path = format!("/{}", file_path.replace("\\", "/"));
    
    match state.update_avatar(&username, &web_path, &current_user).await {
        Ok(_) => (
            StatusCode::OK,
            Json(Response {
                success: true,
                message: "Аватарка обновлена".to_string(),
            }),
        ),
        Err(e) => (
            StatusCode::FORBIDDEN,
            Json(Response {
                success: false,
                message: format!("Ошибка: {}", e),
            }),
        ),
    }
}

pub async fn get_chats(
    Extension(state): Extension<Arc<AppState>>,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> (StatusCode, Json<serde_json::Value>) {
    let username = params.get("username").map(|s| s.as_str()).unwrap_or("");
    
    if username.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": "Username required"
            })),
        );
    }
    
    match state.get_chats(username).await {
        Ok(chats) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "chats": chats
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Ошибка: {}", e)
            })),
        ),
    }
}

pub async fn send_message(
    Extension(state): Extension<Arc<AppState>>,
    Json(msg): Json<serde_json::Value>,
) -> (StatusCode, Json<Response>) {
    let from = msg.get("from").and_then(|v| v.as_str()).unwrap_or("");
    let to = msg.get("to").and_then(|v| v.as_str()).unwrap_or("");
    let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
    
    if from.is_empty() || to.is_empty() || content.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(Response {
                success: false,
                message: "Поля from, to и content обязательны".to_string(),
            }),
        );
    }
    
    match state.save_message(from, to, content).await {
        Ok(_) => (
            StatusCode::CREATED,
            Json(Response {
                success: true,
                message: "Сообщение отправлено".to_string(),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Response {
                success: false,
                message: format!("Ошибка: {}", e),
            }),
        ),
    }
}

pub async fn get_messages(
    Extension(state): Extension<Arc<AppState>>,
    Path((user1, user2)): Path<(String, String)>,
) -> (StatusCode, Json<serde_json::Value>) {
    match state.get_messages(&user1, &user2).await {
        Ok(messages) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "messages": messages
            })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Ошибка: {}", e)
            })),
        ),
    }
}