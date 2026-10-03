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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::testing::{test_db, TestDb};
    use actix_web::body::to_bytes;
    use actix_web::http::StatusCode;
    use actix_web::test::{self, TestRequest};
    use actix_web::App;
    use serde_json::Value;

    async fn call(db: &TestDb) -> (StatusCode, Value) {
        let app = test::init_service(
            App::new()
                .app_data(web::Data::new(db.handle.clone()))
                .route("/api/stats", web::get().to(super::stats)),
        )
        .await;
        let res = test::call_service(&app, TestRequest::get().uri("/api/stats").to_request()).await;
        let status = res.status();
        assert_eq!(
            res.headers().get("cache-control").unwrap(),
            "public, max-age=60"
        );
        let bytes = to_bytes(res.into_body()).await.expect("body");
        (status, serde_json::from_slice(&bytes).expect("json"))
    }

    /// One row per column shape, so every counter has something to count and
    /// every "not set" case is represented too.
    fn seed(db: &TestDb) {
        let c = db.conn();
        for (uid, title_ru, kitsu, shiki, anilist, score, trailer, year) in [
            // full row
            (
                "al:1",
                Some("Атака Титанов"),
                Some(1),
                Some(1),
                Some(1),
                Some(84),
                Some("abc"),
                Some(2013),
            ),
            // every optional column empty
            ("al:2", None, None, None, None, None, None, None),
            // the ones that must NOT be counted: an empty russian title is not
            // a russian title, and an empty trailer id is not a trailer
            (
                "al:3",
                Some(""),
                Some(2),
                None,
                None,
                None,
                Some(""),
                Some(2020),
            ),
        ] {
            c.execute(
                "INSERT INTO anime (uid, anilist_id, title_russian, kitsu_id, shikimori_id,
                    score, trailer_id, start_year, is_adult, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, 1, 1)",
                rusqlite::params![uid, anilist, title_ru, kitsu, shiki, score, trailer, year],
            )
            .expect("insert");
        }
        for (slug, category) in [
            ("action", "genre"),
            ("hentai", "genre"),
            ("studio-ghibli", "studio"),
        ] {
            c.execute(
                "INSERT INTO genres (slug, name_en, category, created_at) VALUES (?1, ?2, ?3, 1)",
                rusqlite::params![slug, slug, category],
            )
            .expect("genre");
        }
    }

    #[actix_web::test]
    async fn an_empty_catalogue_answers_with_zeros_and_no_years() {
        // The endpoint doubles as a health probe, so an empty database has to
        // be a 200 with zeroes rather than an error.
        let db = test_db();
        let (status, v) = call(&db).await;
        assert_eq!(status, StatusCode::OK);
        for key in [
            "anime",
            "with_russian",
            "with_anilist",
            "with_kitsu",
            "with_shikimori",
            "with_score",
            "with_trailer",
            "genres",
            "tags",
            "studios",
            "users",
        ] {
            assert_eq!(v[key], 0, "{} должен быть нулём, ответ: {}", key, v);
        }
        assert!(v["year_min"].is_null(), "года нет: {}", v);
        assert!(v["year_max"].is_null(), "года нет: {}", v);
    }

    #[actix_web::test]
    async fn the_counters_reflect_the_catalogue() {
        let db = test_db();
        seed(&db);
        let (status, v) = call(&db).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v["anime"], 3);
        // The row with an empty russian title is not a translated title, and
        // the row with an empty trailer id is not a trailer. Counting either
        // would make the numbers drift upwards with every sync.
        assert_eq!(v["with_russian"], 1, "ответ: {}", v);
        assert_eq!(v["with_trailer"], 1, "ответ: {}", v);
        assert_eq!(v["with_kitsu"], 2);
        assert_eq!(v["with_shikimori"], 1);
        assert_eq!(v["with_anilist"], 1);
        assert_eq!(v["with_score"], 1);
        assert_eq!(v["year_min"], 2013);
        assert_eq!(v["year_max"], 2020);
    }

    #[actix_web::test]
    async fn the_genre_row_counts_are_split_by_category() {
        // The UI shows three separate chips; one combined number would render
        // studios and tags as if they were genres.
        let db = test_db();
        seed(&db);
        let (_, v) = call(&db).await;
        assert_eq!(v["genres"], 2, "ответ: {}", v);
        assert_eq!(v["tags"], 0);
        assert_eq!(v["studios"], 1);
    }
}
