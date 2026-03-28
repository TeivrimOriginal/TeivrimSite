use axum::{
    extract::{Extension, Path},
    Json,
    http::StatusCode,
};
use bcrypt::{hash, verify, DEFAULT_COST};
use crate::models::*;
use crate::db::Database;

pub async fn register(
    Extension(db): Extension<Database>,
    Json(req): Json<RegisterRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if req.username.is_empty() || req.password.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "message": "Username and password required"
            }))
        );
    }
    
    if let Ok(Some(_)) = db.get_user_by_username(&req.username).await {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "success": false,
                "message": "Username already exists"
            }))
        );
    }
    
    let password_hash = match hash(&req.password, DEFAULT_COST) {
        Ok(hash) => hash,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "success": false,
                    "message": "Failed to hash password"
                }))
            );
        }
    };
    
    match db.create_user(&req, &password_hash).await {
        Ok(user_id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "success": true,
                "user_id": user_id.to_string(),
                "message": "User registered successfully"
            }))
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "message": format!("Registration failed: {}", e)
            }))
        ),
    }
}

pub async fn login(
    Extension(db): Extension<Database>,
    Json(req): Json<LoginRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    let user = match db.get_user_by_username(&req.username).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "success": false,
                    "message": "Invalid credentials"
                }))
            );
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "success": false,
                    "message": format!("Login failed: {}", e)
                }))
            );
        }
    };
    
    match verify(&req.password, &user.password_hash) {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "user_id": user.id.to_string(),
                "message": "Login successful"
            }))
        ),
        _ => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "success": false,
                "message": "Invalid credentials"
            }))
        ),
    }
}

pub async fn send_message(
    Extension(db): Extension<Database>,
    Json(req): Json<SendMessageRequest>,
) -> (StatusCode, Json<serde_json::Value>) {
    if req.content.is_empty() || req.content.len() > 1000 {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "message": "Message must be 1-1000 characters"
            }))
        );
    }
    
    match db.send_message(&req.from_user, &req.to_user, &req.content).await {
        Ok(message_id) => (
            StatusCode::CREATED,
            Json(serde_json::json!({
                "success": true,
                "message_id": message_id.to_string(),
                "message": "Message sent"
            }))
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "message": format!("Error: {}", e)
            }))
        ),
    }
}

pub async fn get_messages(
    Extension(db): Extension<Database>,
    Path((user1, user2)): Path<(String, String)>,
) -> (StatusCode, Json<serde_json::Value>) {
    match db.get_messages(&user1, &user2).await {
        Ok(messages) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "success": true,
                "messages": messages
            }))
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "message": format!("Error: {}", e)
            }))
        ),
    }
}