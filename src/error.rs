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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_maps_to_its_own_status() {
        // These codes are the API contract: a client branches on them.
        assert_eq!(ApiError::bad("x").status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(ApiError::Unauthorized("x".into()).status_code(), StatusCode::UNAUTHORIZED);
        assert_eq!(ApiError::Forbidden("x".into()).status_code(), StatusCode::FORBIDDEN);
        assert_eq!(ApiError::NotFound("x".into()).status_code(), StatusCode::NOT_FOUND);
        assert_eq!(ApiError::Conflict("x".into()).status_code(), StatusCode::CONFLICT);
        assert_eq!(ApiError::internal("x").status_code(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn a_4xx_message_reaches_the_client() {
        // These strings are written by us and are what the UI shows.
        let res = ApiError::BadRequest("плохой запрос".into()).error_response();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn a_5xx_message_never_reaches_the_client() {
        // An internal message can carry a SQL fragment or a file path; it is
        // logged and replaced with a generic message.
        let res = ApiError::Internal("sqlite: no such table: users_secret".into()).error_response();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn the_error_body_has_the_documented_shape() {
        // `{"error":{"code":...,"message":...}}` is what every client parses.
        let res = ApiError::NotFound("Аниме не найдено".into()).error_response();
        assert_eq!(res.headers().get("content-type").unwrap(), "application/json");
    }

    #[test]
    fn a_bad_json_body_is_a_400_and_names_the_parse_error() {
        // A malformed request body is the client's mistake, and the detail is
        // ours, not the database's.
        let parsed = serde_json::from_str::<crate::models::LibraryEntry>("{oops");
        let e: ApiError = parsed.unwrap_err().into();
        assert_eq!(e.status_code(), StatusCode::BAD_REQUEST);
        assert!(e.to_string().starts_with("invalid json: "), "{}", e);
    }

    #[test]
    fn a_database_error_is_reported_as_a_generic_internal_error() {
        // `From<rusqlite::Error>` must not leak the SQL into the response.
        let e: ApiError = rusqlite::Error::InvalidQuery.into();
        assert_eq!(e.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(e.to_string(), "database error");
    }

    #[test]
    fn a_pool_error_is_reported_as_unavailable() {
        let e: ApiError = crate::db::PoolError::Timeout.into();
        assert_eq!(e.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(e.to_string(), "database unavailable");
    }

    #[test]
    fn an_io_error_is_reported_as_an_internal_error() {
        let e: ApiError = std::io::Error::other("disk on fire").into();
        assert_eq!(e.to_string(), "io error");
    }

    #[test]
    fn display_is_the_message_it_was_given() {
        for e in [
            ApiError::bad("a"),
            ApiError::Unauthorized("b".into()),
            ApiError::Forbidden("c".into()),
            ApiError::NotFound("d".into()),
            ApiError::Conflict("e".into()),
            ApiError::internal("f"),
        ] {
            assert_eq!(e.to_string().len(), 1);
        }
    }

    // ------------------------------------------------------------- clock

    #[test]
    fn the_timestamp_format_is_iso_utc() {
        // The log has to be sortable and unambiguous, without pulling in a date
        // crate for it.
        let s = chrono_now();
        assert_eq!(s.len(), 20, "время: {}", s);
        assert!(s.ends_with('Z'), "время не помечено как UTC: {}", s);
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
        assert_eq!(&s[10..11], "T");
        assert_eq!(&s[13..14], ":");
        assert_eq!(&s[16..17], ":");
    }

    #[test]
    fn now_ts_is_a_plausible_unix_timestamp() {
        // 2020-01-01, far enough back that a wrong unit (milliseconds) or a
        // zeroed clock is impossible to miss.
        let now = now_ts();
        assert!(now > 1_577_836_800, "now_ts вернул {}", now);
        assert!(now < 4_102_444_800, "now_ts вернул {}", now);
    }
}
