use actix_web::{http::StatusCode, HttpResponse, ResponseError};
use std::fmt;

/// One error type for the whole API so handlers can just `?` their way out.
#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    Internal(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::BadRequest(m)
            | ApiError::Unauthorized(m)
            | ApiError::Forbidden(m)
            | ApiError::NotFound(m)
            | ApiError::Conflict(m)
            | ApiError::Internal(m) => write!(f, "{}", m),
        }
    }
}

impl std::error::Error for ApiError {}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        // Surface the real reason in the log, never in the response body.
        log_error(&format!("sqlite: {}", e));
        ApiError::Internal("database error".into())
    }
}

impl From<crate::db::PoolError> for ApiError {
    fn from(e: crate::db::PoolError) -> Self {
        log_error(&format!("db pool: {}", e));
        ApiError::Internal("database unavailable".into())
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        ApiError::BadRequest(format!("invalid json: {}", e))
    }
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        log_error(&format!("io: {}", e));
        ApiError::Internal("io error".into())
    }
}

impl ApiError {
    pub fn bad(msg: impl Into<String>) -> Self {
        ApiError::BadRequest(msg.into())
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        ApiError::Internal(msg.into())
    }

    fn status(&self) -> StatusCode {
        match self {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden(_) => StatusCode::FORBIDDEN,
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        self.status()
    }

    fn error_response(&self) -> HttpResponse {
        // 4xx messages are written by us and safe to show. 5xx messages are not:
        // they can contain SQL fragments, so they are replaced with a generic one.
        let (code, message) = match self {
            ApiError::Internal(m) => {
                log_error(m);
                ("internal_error", "internal server error".to_string())
            }
            other => {
                let code = match other {
                    ApiError::BadRequest(_) => "bad_request",
                    ApiError::Unauthorized(_) => "unauthorized",
                    ApiError::Forbidden(_) => "forbidden",
                    ApiError::NotFound(_) => "not_found",
                    ApiError::Conflict(_) => "conflict",
                    _ => "error",
                };
                (code, other.to_string())
            }
        };

        HttpResponse::build(self.status())
            .content_type("application/json")
            .json(serde_json::json!({ "error": { "code": code, "message": message } }))
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// Tiny stderr logger. Deliberately dependency-free: a real `tracing` setup is
/// nice-to-have, not worth another crate in the dependency graph here.
pub fn log_info(msg: &str) {
    println!("[{}] {}", chrono_now(), msg);
}

pub fn log_error(msg: &str) {
    eprintln!("[{}] ERROR: {}", chrono_now(), msg);
}

pub fn log_warn(msg: &str) {
    eprintln!("[{}] WARN:  {}", chrono_now(), msg);
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Minimal UTC formatter (days -> Y-M-D) so we do not need a date crate.
    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let (h, mi, s) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    // Civil-from-days, Howard Hinnant's algorithm.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, h, mi, s)
}

/// Unix seconds, used for every `*_at` column so they stay comparable.
pub fn now_ts() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
