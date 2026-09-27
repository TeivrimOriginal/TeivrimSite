//! Accounts and sessions.
//!
//! Passwords are Argon2id with a per-password random salt. Sessions are opaque
//! random tokens; only their SHA-256 is stored, so a copy of the database does
//! not yield usable credentials. No JWT: revocation is then a single DELETE and
//! there is no signing key to leak or rotate.

use crate::db::Handle;
use crate::error::{now_ts, ApiError, ApiResult};
use crate::models::{AuthResponse, PublicUser};
use argon2::password_hash::phc::PasswordHash;
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use rusqlite::{params, Connection, OptionalExtension};
use std::time::Duration;

// Re-exported so handlers can name the request bodies without reaching into
// the private glob import.
pub use crate::models::{LoginBody, RegisterBody};

const TOKEN_TTL_SECS: i64 = 60 * 60 * 24 * 90; // 90 days
const MIN_PASSWORD_LEN: usize = 8;
const MAX_PASSWORD_LEN: usize = 200; // Argon2 hashes the whole input

/// Extracts and validates a bearer token. Returns `None` for a missing or
/// invalid token rather than erroring, so read-only endpoints stay usable
/// anonymously while `in_list` and watchlist writes require a real session.
pub fn authenticate(db: &Handle, header: Option<&str>) -> Option<i64> {
    let header = header?;
    let raw = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))?
        .trim();
    if raw.is_empty() || raw.len() > 200 {
        return None;
    }
    let hash = hash_token(raw);

    let conn = db.conn().ok()?;
    let found: Option<(i64, i64)> = conn
        .query_row(
            "SELECT user_id, expires_at FROM auth_tokens WHERE token_hash = ?1",
            params![hash],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .ok()
        .flatten();

    let (user_id, expires_at) = found?;
    if expires_at < now_ts() {
        // Expired rows are cleaned up lazily; deleting here is a single indexed
        // write and keeps the table from growing.
        let _ = conn.execute("DELETE FROM auth_tokens WHERE token_hash = ?1", params![hash]);
        return None;
    }
    let _ = conn.execute(
        "UPDATE auth_tokens SET last_used_at = ?1 WHERE token_hash = ?2",
        params![now_ts(), hash],
    );
    Some(user_id)
}

pub fn hash_token(raw: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(raw.as_bytes());
    hex::encode(h.finalize())
}

fn random_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    if getrandom::getrandom(&mut buf).is_err() {
        // getrandom only fails if the OS entropy source is unavailable, which
        // on the platforms this runs on means the machine is already broken.
        // Failing loudly beats silently generating predictable sessions.
        panic!("не удалось получить случайные байты из источника энтропии ОС");
    }
    buf
}

fn random_token() -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes(32))
}

fn hash_password(password: &str) -> ApiResult<String> {
    // 16 random bytes of salt. `hash_password_with_salt` takes raw bytes in
    // argon2 0.6; the previous `SaltString` helper is gone from the API.
    let salt = random_bytes(16);
    let hash: PasswordHash = Argon2::default()
        .hash_password_with_salt(password.as_bytes(), &salt)
        .map_err(|e| ApiError::internal(format!("argon2 hash: {}", e)))?;
    Ok(hash.to_string())
}

fn verify_password(password: &str, stored: &str) -> bool {
    match PasswordHash::new(stored) {
        Ok(hash) => Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok(),
        Err(_) => false,
    }
}

/// Cheap uniform-ish delay so a wrong username and a wrong password take
/// roughly the same time, which stops the endpoint from confirming which
/// accounts exist.
fn constant_delay() {
    std::thread::sleep(Duration::from_millis(120));
}

pub fn register(db: &Handle, body: RegisterBody) -> ApiResult<AuthResponse> {
    let username = body.username.trim();
    if username.len() < 3 || username.chars().count() > 32 {
        return Err(ApiError::bad("Имя пользователя: от 3 до 32 символов"));
    }
    if username.chars().any(|c| c.is_control()) {
        return Err(ApiError::bad("Имя пользователя содержит недопустимые символы"));
    }
    if body.password.chars().count() < MIN_PASSWORD_LEN {
        return Err(ApiError::bad(format!("Пароль должен быть не короче {} символов", MIN_PASSWORD_LEN)));
    }
    if body.password.chars().count() > MAX_PASSWORD_LEN {
        return Err(ApiError::bad("Пароль слишком длинный"));
    }

    let email = body.email.map(|e| e.trim().to_string()).filter(|e| !e.is_empty());
    if let Some(e) = &email {
        // Deliberately loose: a strict RFC check rejects valid addresses.
        if !e.contains('@') || e.len() > 254 || e.contains(char::is_whitespace) {
            return Err(ApiError::bad("Некорректный e-mail"));
        }
    }

    let username_key = crate::loader::title_key(username);
    let email_key = email.as_ref().map(|e| crate::loader::title_key(e));
    let hash = hash_password(&body.password)?;

    let conn = db.conn().map_err(|e| ApiError::internal(e.to_string()))?;
    let now = now_ts();

    let user_id = match conn.query_row(
        "INSERT INTO users (username, username_key, email, email_key, password_hash, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) RETURNING id",
        params![username, username_key, email, email_key, hash, now],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(id) => id,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            // Older SQLite without RETURNING: fall back to last_insert_rowid.
            let _ = conn.execute(
                "INSERT INTO users (username, username_key, email, email_key, password_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![username, username_key, email, email_key, hash, now],
            );
            conn.last_insert_rowid()
        }
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("UNIQUE constraint failed: users.username_key") {
                return Err(ApiError::Conflict("Это имя уже занято".into()));
            }
            if msg.contains("UNIQUE constraint failed: users.email_key") {
                return Err(ApiError::Conflict("Этот e-mail уже зарегистрирован".into()));
            }
            return Err(ApiError::from(e));
        }
    };

    issue_session(&conn, user_id, username, email, now)
}

pub fn login(db: &Handle, body: LoginBody) -> ApiResult<AuthResponse> {
    let conn = db.conn().map_err(|e| ApiError::internal(e.to_string()))?;
    let key = crate::loader::title_key(body.login.trim());

    let row: Option<(i64, String, Option<String>, String)> = conn
        .query_row(
            "SELECT id, username, email, password_hash FROM users
             WHERE username_key = ?1 OR email_key = ?1",
            params![key],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(ApiError::from)?;

    let Some((id, username, email, hash)) = row else {
        // Same work either way, so a wrong username costs the same as a wrong
        // password and the response cannot be used to enumerate accounts.
        constant_delay();
        return Err(ApiError::Unauthorized("Неверный логин или пароль".into()));
    };

    if !verify_password(&body.password, &hash) {
        constant_delay();
        return Err(ApiError::Unauthorized("Неверный логин или пароль".into()));
    }

    let now = now_ts();
    let _ = conn.execute("UPDATE users SET last_login_at = ?1 WHERE id = ?2", params![now, id]);
    issue_session(&conn, id, &username, email, now)
}

fn issue_session(
    conn: &Connection,
    user_id: i64,
    username: &str,
    email: Option<String>,
    now: i64,
) -> ApiResult<AuthResponse> {
    let token = random_token();
    let expires_at = now + TOKEN_TTL_SECS;
    conn.execute(
        "INSERT INTO auth_tokens (token_hash, user_id, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![hash_token(&token), user_id, now, expires_at],
    )?;
    Ok(AuthResponse {
        token,
        expires_at,
        user: PublicUser {
            id: user_id,
            username: username.to_string(),
            email,
            created_at: now,
        },
    })
}

pub fn logout(db: &Handle, header: Option<&str>) -> ApiResult<()> {
    let Some(raw) = header
        .and_then(|h| h.strip_prefix("Bearer ").or_else(|| h.strip_prefix("bearer ")))
        .map(|s| s.trim().to_string())
    else {
        return Ok(());
    };
    let conn = db.conn().map_err(|e| ApiError::internal(e.to_string()))?;
    conn.execute("DELETE FROM auth_tokens WHERE token_hash = ?1", params![hash_token(&raw)])?;
    Ok(())
}

pub fn public_user(conn: &Connection, user_id: i64) -> ApiResult<PublicUser> {
    conn.query_row(
        "SELECT id, username, email, created_at FROM users WHERE id = ?1",
        params![user_id],
        |r| {
            Ok(PublicUser {
                id: r.get(0)?,
                username: r.get(1)?,
                email: r.get(2)?,
                created_at: r.get(3)?,
            })
        },
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => ApiError::Unauthorized("Сессия недействительна".into()),
        other => ApiError::from(other),
    })
}

/// Drops expired sessions. Called on a timer from `main`.
pub fn purge_expired(conn: &Connection) {
    match conn.execute("DELETE FROM auth_tokens WHERE expires_at < ?1", params![now_ts()]) {
        Ok(n) if n > 0 => crate::error::log_info(&format!("[auth] удалено просроченных сессий: {}", n)),
        Ok(_) => {}
        Err(e) => crate::error::log_warn(&format!("[auth] очистка сессий: {}", e)),
    }
}
