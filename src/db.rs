use sqlx::{SqlitePool, sqlite::SqlitePoolOptions, Row};
use std::collections::HashMap;
use std::path::PathBuf;
use crate::models::{UserReg, UserProfile, UserSearchResult};

pub struct AppState {
    pub db: SqlitePool,
    pub sessions: tokio::sync::Mutex<HashMap<String, i64>>,
}

pub async fn init_database() -> SqlitePool {
    // Получаем абсолютный путь к проекту
    let current_dir = std::env::current_dir().expect("Не удалось получить текущую директорию");
    let db_dir = current_dir.join("data");
    
    println!("📁 Текущая директория: {}", current_dir.display());
    println!("📁 Папка БД: {}", db_dir.display());
    
    // Создаем директорию если её нет
    if !db_dir.exists() {
        println!("📁 Создаю папку data...");
        std::fs::create_dir_all(&db_dir).expect("Не удалось создать директорию data");
        println!("✅ Папка data создана");
    } else {
        println!("✅ Папка data существует");
    }
    
    let db_path = db_dir.join("app.db");
    let db_path_str = db_path.to_str().expect("Неверный путь к БД");
    
    println!("📂 Путь к БД: {}", db_path_str);
    
    // Проверяем существование файла БД
    if db_path.exists() {
        println!("✅ Файл БД существует, размер: {} байт", 
                 std::fs::metadata(&db_path).unwrap().len());
    } else {
        println!("⚠️ Файл БД не существует, будет создан при первом подключении");
    }
    
    // Подключаемся к БД
    let pool = match SqlitePoolOptions::new()
        .max_connections(5)
        .connect(db_path_str)
        .await
    {
        Ok(pool) => {
            println!("✅ Подключение к БД установлено");
            pool
        }
        Err(e) => {
            eprintln!("❌ Ошибка подключения к БД: {}", e);
            eprintln!("🔄 Использую БД в памяти...");
            
            SqlitePoolOptions::new()
                .max_connections(5)
                .connect("sqlite::memory:")
                .await
                .expect("Не удалось создать БД в памяти")
        }
    };
    
    // Создаем таблицы
    println!("📝 Создаю таблицы...");
    
    match sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT UNIQUE NOT NULL,
            password TEXT NOT NULL,
            birth_date TEXT NOT NULL,
            email TEXT UNIQUE NOT NULL,
            description TEXT,
            avatar_path TEXT,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )"
    ).execute(&pool).await {
        Ok(_) => println!("✅ Таблица users готова"),
        Err(e) => eprintln!("❌ Ошибка таблицы users: {}", e),
    }
    
    match sqlx::query(
        "CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_user TEXT NOT NULL,
            to_user TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )"
    ).execute(&pool).await {
        Ok(_) => println!("✅ Таблица messages готова"),
        Err(e) => eprintln!("❌ Ошибка таблицы messages: {}", e),
    }
    
    match sqlx::query(
        "CREATE TABLE IF NOT EXISTS chats (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user1 TEXT NOT NULL,
            user2 TEXT NOT NULL,
            last_message TEXT,
            last_message_time DATETIME DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(user1, user2)
        )"
    ).execute(&pool).await {
        Ok(_) => println!("✅ Таблица chats готова"),
        Err(e) => eprintln!("❌ Ошибка таблицы chats: {}", e),
    }
    
    // Проверяем, есть ли пользователи в БД
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&pool)
        .await
        .unwrap_or(0);
    
    println!("📊 В БД {} пользователей", count);
    
    pool
}

// Остальные методы остаются без изменений...
impl AppState {
    pub async fn create_user(&self, user: &UserReg) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO users (username, password, birth_date, email, description) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(&user.username)
        .bind(&user.password)
        .bind(&user.birth_date)
        .bind(&user.email)
        .bind(&user.description)
        .execute(&self.db)
        .await?;
        Ok(())
    }
    
    pub async fn find_user(&self, username: &str, password: &str) -> Result<Option<i64>, sqlx::Error> {
        let row = sqlx::query("SELECT id FROM users WHERE username = ? AND password = ?")
            .bind(username)
            .bind(password)
            .fetch_optional(&self.db)
            .await?;
        Ok(row.map(|r| r.get("id")))
    }
    
    pub async fn get_profile(&self, username: &str, current_user: &str) -> Result<Option<UserProfile>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, username, birth_date, avatar_path, email, description, created_at 
             FROM users WHERE username = ?"
        )
        .bind(username)
        .fetch_optional(&self.db)
            .await?;
        
        Ok(row.map(|r| UserProfile {
            id: r.get("id"),
            username: r.get("username"),
            birth_date: r.get("birth_date"),
            avatar_path: r.get("avatar_path"),
            email: r.get("email"),
            description: r.get("description"),
            created_at: r.get("created_at"),
        }))
    }
    
    pub async fn search_users(&self, query: &str, current_user: &str) -> Result<Vec<UserSearchResult>, sqlx::Error> {
        let search_pattern = format!("%{}%", query);
        let rows = sqlx::query(
            "SELECT username, avatar_path, description FROM users 
             WHERE username LIKE ? AND username != ?
             LIMIT 20"
        )
        .bind(&search_pattern)
        .bind(current_user)
        .fetch_all(&self.db)
        .await?;
        
        Ok(rows.iter().map(|row| UserSearchResult {
            username: row.get("username"),
            avatar_path: row.get("avatar_path"),
            description: row.get("description"),
        }).collect())
    }
    
    pub async fn update_profile(
        &self, 
        username: &str, 
        birth_date: &str, 
        email: &str, 
        description: &str,
        current_user: &str
    ) -> Result<(), sqlx::Error> {
        if username != current_user {
            return Err(sqlx::Error::Protocol("Нельзя редактировать чужой профиль".into()));
        }
        
        sqlx::query(
            "UPDATE users SET birth_date = ?, email = ?, description = ? WHERE username = ?"
        )
        .bind(birth_date)
        .bind(email)
        .bind(description)
        .bind(username)
        .execute(&self.db)
        .await?;
        Ok(())
    }
    
    pub async fn update_avatar(&self, username: &str, avatar_path: &str, current_user: &str) -> Result<(), sqlx::Error> {
        if username != current_user {
            return Err(sqlx::Error::Protocol("Нельзя менять чужую аватарку".into()));
        }
        
        sqlx::query("UPDATE users SET avatar_path = ? WHERE username = ?")
            .bind(avatar_path)
            .bind(username)
            .execute(&self.db)
            .await?;
        Ok(())
    }
    
    pub async fn save_message(&self, from: &str, to: &str, content: &str) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO messages (from_user, to_user, content) VALUES (?, ?, ?)")
            .bind(from)
            .bind(to)
            .bind(content)
            .execute(&self.db)
            .await?;
        
        let (user1, user2) = if from < to { (from, to) } else { (to, from) };
        
        sqlx::query(
            "INSERT INTO chats (user1, user2, last_message, last_message_time) 
             VALUES (?, ?, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(user1, user2) DO UPDATE SET 
             last_message = ?, last_message_time = CURRENT_TIMESTAMP"
        )
        .bind(user1)
        .bind(user2)
        .bind(content)
        .bind(content)
        .execute(&self.db)
        .await?;
        
        Ok(())
    }
    
    pub async fn get_chats(&self, username: &str) -> Result<Vec<serde_json::Value>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT 
                CASE 
                    WHEN user1 = ? THEN user2 
                    ELSE user1 
                END as other_user,
                last_message,
                last_message_time
             FROM chats 
             WHERE user1 = ? OR user2 = ?
             ORDER BY last_message_time DESC"
        )
        .bind(username)
        .bind(username)
        .bind(username)
        .fetch_all(&self.db)
        .await?;
        
        Ok(rows.iter().map(|row| {
            let other_user: String = row.get("other_user");
            serde_json::json!({
                "user": other_user,
                "last_message": row.get::<String, _>("last_message"),
                "last_message_time": row.get::<String, _>("last_message_time"),
            })
        }).collect())
    }
    
    pub async fn get_messages(&self, user1: &str, user2: &str) -> Result<Vec<serde_json::Value>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT from_user, to_user, content, created_at FROM messages 
             WHERE (from_user = ? AND to_user = ?) 
                OR (from_user = ? AND to_user = ?)
             ORDER BY created_at ASC LIMIT 100"
        )
        .bind(user1)
        .bind(user2)
        .bind(user2)
        .bind(user1)
        .fetch_all(&self.db)
        .await?;
        
        Ok(rows.iter().map(|row| {
            serde_json::json!({
                "from": row.get::<String, _>("from_user"),
                "to": row.get::<String, _>("to_user"),
                "content": row.get::<String, _>("content"),
                "time": row.get::<String, _>("created_at"),
            })
        }).collect())
    }
}