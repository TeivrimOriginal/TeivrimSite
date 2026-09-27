use crate::db::Handle;
use crate::error::{ApiError, ApiResult};
use actix_web::{web, HttpResponse, ResponseError};

/// Public catalogue statistics. Drives the "about" strip in the UI and doubles
/// as a cheap health probe: if `anime` is 0 the sync has not run yet.
pub async fn stats(db: web::Data<Handle>) -> HttpResponse {
    let db = db.into_inner();

    let result = web::block(move || {
        let conn = db.conn().map_err(|e| ApiError::internal(e.to_string()))?;
        let one = |sql: &str| -> ApiResult<i64> {
            conn.query_row(sql, [], |r| r.get(0)).map_err(ApiError::from)
        };
        Ok::<serde_json::Value, ApiError>(serde_json::json!({
            "anime": one("SELECT COUNT(*) FROM anime")?,
            "with_russian": one(
                "SELECT COUNT(*) FROM anime WHERE title_russian IS NOT NULL AND title_russian <> ''")?,
            "with_anilist": one("SELECT COUNT(*) FROM anime WHERE anilist_id IS NOT NULL")?,
            "with_kitsu": one("SELECT COUNT(*) FROM anime WHERE kitsu_id IS NOT NULL")?,
            "with_shikimori": one("SELECT COUNT(*) FROM anime WHERE shikimori_id IS NOT NULL")?,
            "with_score": one("SELECT COUNT(*) FROM anime WHERE score IS NOT NULL")?,
            "with_trailer": one(
                "SELECT COUNT(*) FROM anime WHERE trailer_id IS NOT NULL AND trailer_id <> ''")?,
            "genres": one("SELECT COUNT(*) FROM genres WHERE category = 'genre'")?,
            "tags": one("SELECT COUNT(*) FROM genres WHERE category = 'tag'")?,
            "studios": one("SELECT COUNT(*) FROM genres WHERE category = 'studio'")?,
            "users": one("SELECT COUNT(*) FROM users")?,
            "year_min": conn.query_row(
                "SELECT MIN(start_year) FROM anime WHERE start_year IS NOT NULL",
                [], |r| r.get::<_, Option<i64>>(0))?,
            "year_max": conn.query_row(
                "SELECT MAX(start_year) FROM anime WHERE start_year IS NOT NULL",
                [], |r| r.get::<_, Option<i64>>(0))?,
        }))
    })
    .await;

    match result {
        Ok(Ok(v)) => HttpResponse::Ok()
            .insert_header(("cache-control", "public, max-age=60"))
            .json(v),
        Ok(Err(e)) => e.error_response(),
        Err(_) => ApiError::internal("stats worker failed").error_response(),
    }
}
