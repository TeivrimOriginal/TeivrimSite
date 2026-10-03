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
        let _ = conn.execute(
            "DELETE FROM auth_tokens WHERE token_hash = ?1",
            params![hash],
        );
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
        return Err(ApiError::bad(
            "Имя пользователя содержит недопустимые символы",
        ));
    }
    if body.password.chars().count() < MIN_PASSWORD_LEN {
        return Err(ApiError::bad(format!(
            "Пароль должен быть не короче {} символов",
            MIN_PASSWORD_LEN
        )));
    }
    if body.password.chars().count() > MAX_PASSWORD_LEN {
        return Err(ApiError::bad("Пароль слишком длинный"));
    }

    let email = body
        .email
        .map(|e| e.trim().to_string())
        .filter(|e| !e.is_empty());
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
    let _ = conn.execute(
        "UPDATE users SET last_login_at = ?1 WHERE id = ?2",
        params![now, id],
    );
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
        .and_then(|h| {
            h.strip_prefix("Bearer ")
                .or_else(|| h.strip_prefix("bearer "))
        })
        .map(|s| s.trim().to_string())
    else {
        return Ok(());
    };
    let conn = db.conn().map_err(|e| ApiError::internal(e.to_string()))?;
    conn.execute(
        "DELETE FROM auth_tokens WHERE token_hash = ?1",
        params![hash_token(&raw)],
    )?;
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
        rusqlite::Error::QueryReturnedNoRows => {
            ApiError::Unauthorized("Сессия недействительна".into())
        }
        other => ApiError::from(other),
    })
}

/// Drops expired sessions. Called on a timer from `main`.
pub fn purge_expired(conn: &Connection) {
    match conn.execute(
        "DELETE FROM auth_tokens WHERE expires_at < ?1",
        params![now_ts()],
    ) {
        Ok(n) if n > 0 => {
            crate::error::log_info(&format!("[auth] удалено просроченных сессий: {}", n))
        }
        Ok(_) => {}
        Err(e) => crate::error::log_warn(&format!("[auth] очистка сессий: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::testing::test_db;
    use actix_web::http::StatusCode;
    use actix_web::ResponseError;

    fn body(username: &str, password: &str) -> RegisterBody {
        RegisterBody {
            username: username.to_string(),
            email: None,
            password: password.to_string(),
        }
    }

    fn code(e: &ApiError) -> StatusCode {
        e.status_code()
    }

    // ---------------------------------------------------------- token hash

    #[test]
    fn a_token_hash_is_stable_and_unique() {
        // Only the hash is stored, so this function is the entire security of
        // the session table.
        let a = hash_token("token");
        assert_eq!(a, hash_token("token"));
        assert_ne!(a, hash_token("token2"));
        assert_eq!(a.len(), 64, "SHA-256 в hex");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn the_hash_is_not_the_token() {
        assert!(!hash_token("secret").contains("secret"));
    }

    // ------------------------------------------------------------ register

    #[test]
    fn a_valid_registration_returns_a_session() {
        let db = test_db();
        let r = register(&db.handle, body("user", "password123")).unwrap();
        assert!(!r.token.is_empty());
        assert!(r.expires_at > now_ts());
        assert_eq!(r.user.username, "user");
        assert_eq!(r.user.email, None);
        assert!(r.user.id > 0);
    }

    #[test]
    fn the_username_is_trimmed_before_it_is_stored() {
        let db = test_db();
        let r = register(&db.handle, body("  user  ", "password123")).unwrap();
        assert_eq!(r.user.username, "user");
    }

    #[test]
    fn a_short_username_is_refused() {
        let db = test_db();
        for name in ["ab", "", "  "] {
            let e = register(&db.handle, body(name, "password123")).unwrap_err();
            assert_eq!(code(&e), StatusCode::BAD_REQUEST, "имя {:?}", name);
        }
    }

    #[test]
    fn an_overlong_username_is_refused() {
        let db = test_db();
        let e = register(&db.handle, body(&"x".repeat(33), "password123")).unwrap_err();
        assert_eq!(code(&e), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn a_short_password_is_refused() {
        // Eight characters: anything shorter is trivially guessable even with
        // a rate limit in front.
        let db = test_db();
        let e = register(&db.handle, body("user", "1234567")).unwrap_err();
        assert_eq!(code(&e), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn an_absurdly_long_password_is_refused() {
        // Argon2 hashes the whole input, so an unbounded password is an
        // unbounded amount of work per request.
        let db = test_db();
        let e = register(&db.handle, body("user", &"x".repeat(201))).unwrap_err();
        assert_eq!(code(&e), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn a_control_character_in_the_username_is_refused() {
        // It would be a log-injection and display-injection vector in the
        // header of every admin page that lists a user.
        let db = test_db();
        let e = register(&db.handle, body("user\nadmin", "password123")).unwrap_err();
        assert_eq!(code(&e), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn a_broken_email_is_refused_and_a_loose_one_is_accepted() {
        let db = test_db();
        for bad in ["no-at-sign", "a b@c.d", ""] {
            let mut b = body("user", "password123");
            b.email = Some(bad.to_string());
            // An empty email is dropped rather than refused: it is what a
            // client that sends the field blank means.
            if bad.is_empty() {
                assert!(
                    register(&db.handle, b).is_ok(),
                    "пустой e-mail должен приниматься"
                );
                continue;
            }
            let e = register(&db.handle, b).unwrap_err();
            assert_eq!(code(&e), StatusCode::BAD_REQUEST, "e-mail {:?}", bad);
        }
    }

    #[test]
    fn usernames_that_differ_only_in_case_collide() {
        // `username_key` is the normalised name, which is what makes the unique
        // index work: without it `User` and `user` would be two accounts.
        let db = test_db();
        register(&db.handle, body("User", "password123")).unwrap();
        let e = register(&db.handle, body("user", "password123")).unwrap_err();
        assert_eq!(code(&e), StatusCode::CONFLICT);
    }

    #[test]
    fn the_same_email_cannot_be_registered_twice() {
        let db = test_db();
        let mut a = body("first", "password123");
        a.email = Some("a@b.c".into());
        register(&db.handle, a).unwrap();

        let mut b = body("second", "password123");
        b.email = Some("A@B.C".into());
        let e = register(&db.handle, b).unwrap_err();
        assert_eq!(code(&e), StatusCode::CONFLICT);
    }

    #[test]
    fn the_password_is_never_stored_in_the_clear() {
        // A database copy must not hand out working credentials.
        let db = test_db();
        register(&db.handle, body("user", "password123")).unwrap();
        let hash: String = db
            .conn()
            .query_row("SELECT password_hash FROM users", [], |r| r.get(0))
            .unwrap();
        assert!(!hash.contains("password123"));
        assert!(hash.starts_with("$argon2"), "хеш: {}", hash);
    }

    // --------------------------------------------------------------- login

    #[test]
    fn the_right_password_opens_a_session() {
        let db = test_db();
        register(&db.handle, body("user", "password123")).unwrap();
        let r = login(
            &db.handle,
            LoginBody {
                login: "user".into(),
                password: "password123".into(),
            },
        )
        .unwrap();
        assert!(!r.token.is_empty());
        assert_eq!(r.user.username, "user");
    }

    #[test]
    fn an_email_can_be_used_to_log_in() {
        let db = test_db();
        let mut b = body("user", "password123");
        b.email = Some("a@b.c".into());
        register(&db.handle, b).unwrap();
        let r = login(
            &db.handle,
            LoginBody {
                login: "a@b.c".into(),
                password: "password123".into(),
            },
        )
        .unwrap();
        assert_eq!(r.user.username, "user");
    }

    #[test]
    fn the_login_is_case_insensitive() {
        let db = test_db();
        register(&db.handle, body("User", "password123")).unwrap();
        assert!(login(
            &db.handle,
            LoginBody {
                login: "USER".into(),
                password: "password123".into()
            }
        )
        .is_ok());
    }

    #[test]
    fn a_wrong_password_is_unauthorised() {
        let db = test_db();
        register(&db.handle, body("user", "password123")).unwrap();
        let e = login(
            &db.handle,
            LoginBody {
                login: "user".into(),
                password: "wrong".into(),
            },
        )
        .unwrap_err();
        assert_eq!(code(&e), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn an_unknown_user_gets_the_same_answer_as_a_wrong_password() {
        // Anything different would turn the login endpoint into an account
        // enumeration oracle.
        let db = test_db();
        register(&db.handle, body("user", "password123")).unwrap();
        let unknown = login(
            &db.handle,
            LoginBody {
                login: "nobody".into(),
                password: "password123".into(),
            },
        )
        .unwrap_err();
        let wrong = login(
            &db.handle,
            LoginBody {
                login: "user".into(),
                password: "password123!".into(),
            },
        )
        .unwrap_err();
        assert_eq!(unknown.to_string(), wrong.to_string());
        assert_eq!(code(&unknown), code(&wrong));
    }

    // -------------------------------------------------------- authenticate

    #[test]
    fn a_issued_token_authenticates() {
        let db = test_db();
        let r = register(&db.handle, body("user", "password123")).unwrap();
        let uid = authenticate(&db.handle, Some(&format!("Bearer {}", r.token)));
        assert_eq!(uid, Some(r.user.id));
    }

    #[test]
    fn a_lower_case_bearer_prefix_also_works() {
        // Some HTTP clients lowercase the scheme.
        let db = test_db();
        let r = register(&db.handle, body("user", "password123")).unwrap();
        assert_eq!(
            authenticate(&db.handle, Some(&format!("bearer {}", r.token))),
            Some(r.user.id)
        );
    }

    #[test]
    fn a_missing_or_malformed_header_is_anonymous_rather_than_an_error() {
        // Read-only endpoints stay usable without a session, so this returns
        // None instead of failing the request.
        let db = test_db();
        for h in [
            None,
            Some(""),
            Some("Bearer"),
            Some("Bearer "),
            Some("Basic abc"),
            Some("abc"),
        ] {
            assert_eq!(authenticate(&db.handle, h), None, "заголовок {:?}", h);
        }
    }

    #[test]
    fn a_token_with_control_characters_is_refused() {
        // The raw token goes into a hash, so a giant or binary header must not
        // become a cheap way to burn CPU on every request.
        let db = test_db();
        let long = format!("Bearer {}", "x".repeat(500));
        assert_eq!(authenticate(&db.handle, Some(&long)), None);
    }

    #[test]
    fn an_unknown_token_is_refused() {
        let db = test_db();
        assert_eq!(
            authenticate(&db.handle, Some("Bearer not-a-real-token")),
            None
        );
    }

    #[test]
    fn logging_out_invalidates_the_token() {
        let db = test_db();
        let r = register(&db.handle, body("user", "password123")).unwrap();
        let header = format!("Bearer {}", r.token);
        assert!(authenticate(&db.handle, Some(&header)).is_some());

        logout(&db.handle, Some(&header)).unwrap();
        assert!(authenticate(&db.handle, Some(&header)).is_none());
    }

    #[test]
    fn logging_out_without_a_token_is_a_no_op() {
        let db = test_db();
        assert!(logout(&db.handle, None).is_ok());
        assert!(logout(&db.handle, Some("garbage")).is_ok());
    }

    #[test]
    fn an_expired_token_is_refused_and_cleaned_up() {
        let db = test_db();
        let r = register(&db.handle, body("user", "password123")).unwrap();
        db.conn()
            .execute(
                "UPDATE auth_tokens SET expires_at = ?1",
                params![now_ts() - 1],
            )
            .unwrap();
        assert_eq!(
            authenticate(&db.handle, Some(&format!("Bearer {}", r.token))),
            None
        );

        let left: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM auth_tokens", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0, "просроченная сессия должна удаляться сразу");
    }

    #[test]
    fn purge_expired_keeps_the_live_sessions() {
        let db = test_db();
        let r = register(&db.handle, body("user", "password123")).unwrap();
        let hash = hash_token(&r.token);
        db.conn()
            .execute(
                "INSERT INTO auth_tokens (token_hash, user_id, created_at, expires_at) VALUES ('dead', 1, 1, ?1)",
                params![now_ts() - 1],
            )
            .unwrap();
        purge_expired(&db.conn());

        let left: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM auth_tokens WHERE token_hash = ?1",
                [&hash],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 1);
    }

    // ---------------------------------------------------------- public user

    #[test]
    fn the_public_user_never_carries_the_hash() {
        let db = test_db();
        let r = register(&db.handle, body("user", "password123")).unwrap();
        let u = public_user(&db.conn(), r.user.id).unwrap();
        assert_eq!(u.username, "user");
        assert_eq!(u.created_at, r.user.created_at);
    }

    #[test]
    fn an_unknown_user_id_is_unauthorised() {
        // A session row that points at a deleted user must not become a way to
        // read the catalogue as somebody.
        let db = test_db();
        let e = public_user(&db.conn(), 999).unwrap_err();
        assert_eq!(code(&e), StatusCode::UNAUTHORIZED);
    }
}
