use axum::{
    routing::{post, get},
    Router,
    Json,
    extract::{Extension, Multipart, Path},
    response::Html,
    http::StatusCode,
};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions, Row};
use serde::{Serialize, Deserialize};
use std::net::SocketAddr;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::fs;
use uuid::Uuid;

// ============= МОДЕЛИ =============
#[derive(Debug, Deserialize)]
struct UserReg {
    username: String,
    password: String,
    birth_date: String,
    email: String,
    description: String,
}

#[derive(Debug, Deserialize)]
struct UserLogin {
    username: String,
    password: String,
}

#[derive(Debug, Serialize)]
struct Response {
    success: bool,
    message: String,
}

#[derive(Debug, Serialize, Clone)]
struct UserProfile {
    id: i64,
    username: String,
    birth_date: String,
    avatar_path: Option<String>,
    email: String,
    description: String,
    created_at: String,
}

// ============= БАЗА ДАННЫХ =============
struct AppState {
    db: SqlitePool,
    sessions: tokio::sync::Mutex<HashMap<String, i64>>, // session_token -> user_id
}

// ============= ГЛАВНАЯ ФУНКЦИЯ =============
#[tokio::main]
async fn main() {
    // 1. СОЗДАНИЕ БД В ФАЙЛЕ (для сохранения данных)
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect("sqlite:app.db")
        .await
        .expect("Не удалось создать БД");
    
    // 2. СОЗДАНИЕ ТАБЛИЦ
    sqlx::query(
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
    ).execute(&pool).await.expect("Не удалось создать таблицу users");
    
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_user TEXT NOT NULL,
            to_user TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )"
    ).execute(&pool).await.expect("Не удалось создать таблицу messages");
    
    println!("✅ База данных SQLite создана");
    println!("✅ Таблицы users и messages созданы");
    
    // Создаем директорию для аватарок
    fs::create_dir_all("uploads/avatars").await.unwrap_or_default();
    
    // 3. СОСТОЯНИЕ ПРИЛОЖЕНИЯ
    let state = Arc::new(AppState { 
        db: pool,
        sessions: tokio::sync::Mutex::new(HashMap::new()),
    });
    
    // 4. МАРШРУТЫ
    let app = Router::new()
        .route("/", get(root))
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/profile/:username", get(profile_page))
        .route("/api/profile/:username", get(get_profile))
        .route("/api/profile/update", post(update_profile))
        .route("/api/profile/avatar", post(upload_avatar))
        .route("/message", post(send_message))
        .route("/messages/:user1/:user2", get(get_messages))
        .layer(Extension(state));
    
    // 5. ЗАПУСК СЕРВЕРА
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("🚀 Сервер запущен: http://{}", addr);
    println!("📝 Доступные эндпоинты:");
    println!("   GET  /");
    println!("   POST /register");
    println!("   POST /login");
    println!("   POST /logout");
    println!("   GET  /profile/:username");
    println!("   POST /api/profile/update");
    println!("   POST /api/profile/avatar");
    println!("   POST /message");
    println!("   GET  /messages/:user1/:user2");
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ============= HTML СТРАНИЦЫ =============

async fn root() -> Html<String> {
    Html(format!(r#"
<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>Messenger</title>
</head>
<body>
    <h1>Добро пожаловать в Messenger</h1>
    
    <div id="auth">
        <h2>Вход</h2>
        <form id="loginForm">
            <input type="text" id="login_username" placeholder="Username" required><br>
            <input type="password" id="login_password" placeholder="Password" required><br>
            <button type="submit">Войти</button>
        </form>
        
        <h2>Регистрация</h2>
        <form id="registerForm">
            <input type="text" id="reg_username" placeholder="Username" required><br>
            <input type="password" id="reg_password" placeholder="Password" required><br>
            <input type="date" id="reg_birth_date" required><br>
            <input type="email" id="reg_email" placeholder="Email" required><br>
            <textarea id="reg_description" placeholder="Описание"></textarea><br>
            <button type="submit">Зарегистрироваться</button>
        </form>
    </div>
    
    <div id="loggedIn" style="display:none">
        <p>Вы вошли как: <span id="currentUser"></span></p>
        <button onclick="logout()">Выйти</button>
        <button onclick="goToProfile()">Мой профиль</button>
    </div>

    <script>
        let currentUsername = null;
        
        document.getElementById('loginForm').onsubmit = async (e) => {{
            e.preventDefault();
            const username = document.getElementById('login_username').value;
            const password = document.getElementById('login_password').value;
            
            const res = await fetch('/login', {{
                method: 'POST',
                headers: {{'Content-Type': 'application/json'}},
                body: JSON.stringify({{username, password}})
            }});
            
            const data = await res.json();
            if (data.success) {{
                currentUsername = username;
                document.getElementById('auth').style.display = 'none';
                document.getElementById('loggedIn').style.display = 'block';
                document.getElementById('currentUser').innerText = username;
                alert('Вход выполнен!');
            }} else {{
                alert(data.message);
            }}
        }};
        
        document.getElementById('registerForm').onsubmit = async (e) => {{
            e.preventDefault();
            const username = document.getElementById('reg_username').value;
            const password = document.getElementById('reg_password').value;
            const birth_date = document.getElementById('reg_birth_date').value;
            const email = document.getElementById('reg_email').value;
            const description = document.getElementById('reg_description').value;
            
            const res = await fetch('/register', {{
                method: 'POST',
                headers: {{'Content-Type': 'application/json'}},
                body: JSON.stringify({{username, password, birth_date, email, description}})
            }});
            
            const data = await res.json();
            alert(data.message);
            if (data.success) {{
                document.getElementById('login_username').value = username;
                document.getElementById('loginForm').onsubmit(new Event('submit'));
            }}
        }};
        
        async function logout() {{
            await fetch('/logout', {{method: 'POST'}});
            currentUsername = null;
            document.getElementById('auth').style.display = 'block';
            document.getElementById('loggedIn').style.display = 'none';
        }}
        
        function goToProfile() {{
            window.location.href = `/profile/${{currentUsername}}`;
        }}
    </script>
</body>
</html>
"#))
}

async fn profile_page(Path(username): Path<String>) -> Html<String> {
    Html(format!(r#"
<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>Профиль - {}</title>
</head>
<body>
    <div id="profile"></div>
    <button onclick="location.href='/'">На главную</button>
    
    <div id="editForm" style="display:none; margin-top:20px; border:1px solid #ccc; padding:10px;">
        <h3>Редактировать профиль</h3>
        <form id="updateForm">
            <input type="date" id="edit_birth_date" placeholder="Дата рождения"><br>
            <input type="email" id="edit_email" placeholder="Email"><br>
            <textarea id="edit_description" placeholder="Описание"></textarea><br>
            <button type="submit">Сохранить</button>
        </form>
        
        <h4>Сменить аватарку</h4>
        <form id="avatarForm" enctype="multipart/form-data">
            <input type="file" id="avatar" name="avatar" accept="image/*"><br>
            <button type="submit">Загрузить</button>
        </form>
    </div>
    
    <button id="editBtn" onclick="showEditForm()">Редактировать профиль</button>

    <script>
        let currentUser = null;
        const username = '{}';
        
        async function loadProfile() {{
            const res = await fetch(`/api/profile/${{username}}`);
            const data = await res.json();
            
            if (data.success) {{
                const p = data.profile;
                currentUser = p;
                document.getElementById('profile').innerHTML = `
                    <h1>${{p.username}}</h1>
                    <div>
                        ${{p.avatar_path ? `<img src="/${{p.avatar_path}}" width="150" height="150" style="border-radius:50%">` : '<div style="width:150px;height:150px;background:#ccc;border-radius:50%;display:flex;align-items:center;justify-content:center">Нет фото</div>'}}
                    </div>
                    <p><strong>Дата рождения:</strong> ${{p.birth_date}}</p>
                    <p><strong>Email:</strong> ${{p.email}}</p>
                    <p><strong>Описание:</strong> ${{p.description || 'Нет описания'}}</p>
                    <p><strong>Зарегистрирован:</strong> ${{p.created_at}}</p>
                `;
                
                document.getElementById('edit_birth_date').value = p.birth_date;
                document.getElementById('edit_email').value = p.email;
                document.getElementById('edit_description').value = p.description || '';
            }}
        }}
        
        async function showEditForm() {{
            document.getElementById('editForm').style.display = 'block';
        }}
        
        document.getElementById('updateForm').onsubmit = async (e) => {{
            e.preventDefault();
            const birth_date = document.getElementById('edit_birth_date').value;
            const email = document.getElementById('edit_email').value;
            const description = document.getElementById('edit_description').value;
            
            const res = await fetch('/api/profile/update', {{
                method: 'POST',
                headers: {{'Content-Type': 'application/json'}},
                body: JSON.stringify({{username, birth_date, email, description}})
            }});
            
            const data = await res.json();
            alert(data.message);
            if (data.success) {{
                loadProfile();
                document.getElementById('editForm').style.display = 'none';
            }}
        }};
        
        document.getElementById('avatarForm').onsubmit = async (e) => {{
            e.preventDefault();
            const formData = new FormData();
            formData.append('username', username);
            formData.append('avatar', document.getElementById('avatar').files[0]);
            
            const res = await fetch('/api/profile/avatar', {{
                method: 'POST',
                body: formData
            }});
            
            const data = await res.json();
            alert(data.message);
            if (data.success) {{
                loadProfile();
            }}
        }};
        
        loadProfile();
    </script>
</body>
</html>
"#, username, username))
}

// ============= API ОБРАБОТЧИКИ =============

// Регистрация пользователя
async fn register(
    Extension(state): Extension<Arc<AppState>>,
    Json(user): Json<UserReg>,
) -> (StatusCode, Json<Response>) {
    match sqlx::query(
        "INSERT INTO users (username, password, birth_date, email, description) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(&user.username)
    .bind(&user.password)
    .bind(&user.birth_date)
    .bind(&user.email)
    .bind(&user.description)
    .execute(&state.db)
    .await
    {
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

// Вход пользователя
async fn login(
    Extension(state): Extension<Arc<AppState>>,
    Json(login): Json<UserLogin>,
) -> (StatusCode, Json<Response>) {
    let row = sqlx::query("SELECT id FROM users WHERE username = ? AND password = ?")
        .bind(&login.username)
        .bind(&login.password)
        .fetch_optional(&state.db)
        .await;
    
    match row {
        Ok(Some(row)) => {
            let user_id: i64 = row.get("id");
            let session_token = Uuid::new_v4().to_string();
            
            let mut sessions = state.sessions.lock().await;
            sessions.insert(session_token, user_id);
            
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

// Выход
async fn logout(
    Extension(state): Extension<Arc<AppState>>,
) -> (StatusCode, Json<Response>) {
    // В реальном приложении нужно получать токен из куки/заголовка
    // Для простоты просто очищаем все сессии (неправильно, но для демо)
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

// Получение профиля
async fn get_profile(
    Extension(state): Extension<Arc<AppState>>,
    Path(username): Path<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    let row = sqlx::query(
        "SELECT id, username, birth_date, avatar_path, email, description, created_at 
         FROM users WHERE username = ?"
    )
    .bind(&username)
    .fetch_optional(&state.db)
    .await;
    
    match row {
        Ok(Some(row)) => {
            let profile = UserProfile {
                id: row.get("id"),
                username: row.get("username"),
                birth_date: row.get("birth_date"),
                avatar_path: row.get("avatar_path"),
                email: row.get("email"),
                description: row.get("description"),
                created_at: row.get("created_at"),
            };
            
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "profile": profile
                })),
            )
        }
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

// Обновление профиля
async fn update_profile(
    Extension(state): Extension<Arc<AppState>>,
    Json(data): Json<serde_json::Value>,
) -> (StatusCode, Json<Response>) {
    let username = data.get("username").and_then(|v| v.as_str()).unwrap_or("");
    let birth_date = data.get("birth_date").and_then(|v| v.as_str()).unwrap_or("");
    let email = data.get("email").and_then(|v| v.as_str()).unwrap_or("");
    let description = data.get("description").and_then(|v| v.as_str()).unwrap_or("");
    
    if username.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(Response {
                success: false,
                message: "Username обязателен".to_string(),
            }),
        );
    }
    
    match sqlx::query(
        "UPDATE users SET birth_date = ?, email = ?, description = ? WHERE username = ?"
    )
    .bind(birth_date)
    .bind(email)
    .bind(description)
    .bind(username)
    .execute(&state.db)
    .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(Response {
                success: true,
                message: "Профиль обновлен".to_string(),
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

// Загрузка аватарки
async fn upload_avatar(
    Extension(state): Extension<Arc<AppState>>,
    mut multipart: Multipart,
) -> (StatusCode, Json<Response>) {
    let mut username = String::new();
    let mut avatar_data: Option<Vec<u8>> = None;
    let mut avatar_filename = String::new();
    
    while let Ok(Some(mut field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "username" {
            if let Ok(text) = field.text().await {
                username = text;
            }
        } else if name == "avatar" {
            if let Ok(data) = field.bytes().await {
                let file_name = field.file_name().unwrap_or("avatar.jpg").to_string();
                avatar_data = Some(data.to_vec());
                avatar_filename = format!("{}_{}", username, file_name);
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
    
    let file_path = format!("uploads/avatars/{}", avatar_filename);
    let file_path_clone = file_path.clone();
    
    // Сохраняем файл
    if let Err(e) = fs::write(&file_path, avatar_data.unwrap()).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Response {
                success: false,
                message: format!("Ошибка сохранения файла: {}", e),
            }),
        );
    }
    
    // Обновляем путь в БД
    match sqlx::query("UPDATE users SET avatar_path = ? WHERE username = ?")
        .bind(&file_path_clone)
        .bind(&username)
        .execute(&state.db)
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(Response {
                success: true,
                message: "Аватарка обновлена".to_string(),
            }),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Response {
                success: false,
                message: format!("Ошибка обновления БД: {}", e),
            }),
        ),
    }
}

// Отправка сообщения
async fn send_message(
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
    
    match sqlx::query("INSERT INTO messages (from_user, to_user, content) VALUES (?, ?, ?)")
        .bind(from)
        .bind(to)
        .bind(content)
        .execute(&state.db)
        .await
    {
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

// Получение сообщений между пользователями
async fn get_messages(
    Extension(state): Extension<Arc<AppState>>,
    Path((user1, user2)): Path<(String, String)>,
) -> (StatusCode, Json<serde_json::Value>) {
    let rows = sqlx::query(
        "SELECT from_user, to_user, content, created_at FROM messages 
         WHERE (from_user = ? AND to_user = ?) 
            OR (from_user = ? AND to_user = ?)
         ORDER BY created_at DESC LIMIT 50"
    )
    .bind(&user1)
    .bind(&user2)
    .bind(&user2)
    .bind(&user1)
    .fetch_all(&state.db)
    .await;
    
    match rows {
        Ok(rows) => {
            let messages: Vec<serde_json::Value> = rows
                .iter()
                .map(|row| {
                    serde_json::json!({
                        "from": row.get::<String, _>("from_user"),
                        "to": row.get::<String, _>("to_user"),
                        "content": row.get::<String, _>("content"),
                        "time": row.get::<String, _>("created_at"),
                    })
                })
                .collect();
            
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "messages": messages
                })),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "success": false,
                "error": format!("Ошибка: {}", e)
            })),
        ),
    }
}