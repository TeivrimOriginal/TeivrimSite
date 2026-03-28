use sqlx::{SqlitePool, sqlite::SqlitePoolOptions, Row};
use anyhow::Result;
use uuid::Uuid;
use chrono::Utc;
use crate::models::*;

#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    pub async fn new() -> Result<Self> {
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect("sqlite:social.db")
            .await?;
        
        Ok(Database { pool })
    }
    
    pub async fn init_schema(&self) -> Result<()> {
        // Таблица пользователей
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS users (
                id TEXT PRIMARY KEY,
                username TEXT UNIQUE NOT NULL,
                email TEXT NOT NULL,
                password_hash TEXT NOT NULL,
                created_at TEXT NOT NULL
            )"
        ).execute(&self.pool).await?;
        
        // Таблица сообщений
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                from_user TEXT NOT NULL,
                to_user TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp TEXT NOT NULL
            )"
        ).execute(&self.pool).await?;
        
        // Индексы для быстрого поиска
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_messages_from_to 
             ON messages(from_user, to_user)"
        ).execute(&self.pool).await?;
        
        println!("База данных инициализирована");
        Ok(())
    }
    
    pub async fn create_user(&self, req: &RegisterRequest, password_hash: &str) -> Result<Uuid> {
        let user_id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        
        sqlx::query(
            "INSERT INTO users (id, username, email, password_hash, created_at) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(user_id.to_string())
        .bind(&req.username)
        .bind(&req.email)
        .bind(password_hash)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        
        Ok(user_id)
    }
    
    pub async fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
        let row = sqlx::query("SELECT id, username, email, password_hash, created_at FROM users WHERE username = ?")
            .bind(username)
            .fetch_optional(&self.pool)
            .await?;
        
        if let Some(row) = row {
            let user = User {
                id: Uuid::parse_str(row.get::<String, _>("id").as_str())?,
                username: row.get("username"),
                email: row.get("email"),
                password_hash: row.get("password_hash"),
                created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<String, _>("created_at"))
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            };
            Ok(Some(user))
        } else {
            Ok(None)
        }
    }
    
    pub async fn send_message(&self, from_user: &str, to_user: &str, content: &str) -> Result<Uuid> {
        let message_id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        
        sqlx::query(
            "INSERT INTO messages (id, from_user, to_user, content, timestamp) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(message_id.to_string())
        .bind(from_user)
        .bind(to_user)
        .bind(content)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        
        Ok(message_id)
    }
    
    pub async fn get_messages(&self, user1: &str, user2: &str) -> Result<Vec<Message>> {
        let rows = sqlx::query(
            "SELECT id, from_user, to_user, content, timestamp FROM messages 
             WHERE (from_user = ? AND to_user = ?) 
                OR (from_user = ? AND to_user = ?)
             ORDER BY timestamp DESC LIMIT 50"
        )
        .bind(user1)
        .bind(user2)
        .bind(user2)
        .bind(user1)
        .fetch_all(&self.pool)
        .await?;
        
        let mut messages = Vec::new();
        for row in rows {
            messages.push(Message {
                id: Uuid::parse_str(row.get::<String, _>("id").as_str())?,
                from_user: row.get("from_user"),
                to_user: row.get("to_user"),
                content: row.get("content"),
                timestamp: chrono::DateTime::parse_from_rfc3339(&row.get::<String, _>("timestamp"))
                    .unwrap()
                    .with_timezone(&chrono::Utc),
            });
        }
        
        Ok(messages)
    }
}