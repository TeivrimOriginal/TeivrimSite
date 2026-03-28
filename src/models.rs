use serde::{Serialize, Deserialize};

#[derive(Debug, Deserialize)]
pub struct UserReg {
    pub username: String,
    pub password: String,
    pub birth_date: String,
    pub email: String,
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct UserLogin {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct UserProfile {
    pub id: i64,
    pub username: String,
    pub birth_date: String,
    pub avatar_path: Option<String>,
    pub email: String,
    pub description: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct UserSearchResult {
    pub username: String,
    pub avatar_path: Option<String>,
    pub description: String,
}