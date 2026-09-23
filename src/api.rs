use actix_web::{web, HttpResponse, Responder};
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db;

#[derive(Deserialize)]
pub struct PageQuery {
    pub page: Option<u32>,
    pub q: Option<String>,
    pub sort: Option<String>,
    pub format: Option<String>,
    pub status: Option<String>,
    pub season: Option<String>,
    pub year_from: Option<i64>,
    pub year_to: Option<i64>,
    pub score_from: Option<i64>,
    pub score_to: Option<i64>,
    pub genre: Option<String>,
    pub country: Option<String>,
    pub adult: Option<String>,
    pub licensed: Option<String>,
    pub has_ru: Option<String>,
}

#[derive(Serialize)]
pub struct AnimeListItem {
    pub id: i64,
    pub title: String,
    pub title_russian: Option<String>,
    pub title_ru_machine: Option<String>,
    pub title_english: Option<String>,
    pub cover: String,
    pub score: i64,
    pub year: Option<i64>,
    pub format: Option<String>,
    pub status: Option<String>,
    pub episodes: Option<i64>,
}

fn build_filters(q: &PageQuery) -> (String, Vec<Box<dyn rusqlite::ToSql>>) {
    let mut conds: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(s) = q.q.as_ref() {
        let s = s.trim();
        if !s.is_empty() {
            let like = format!("%{}%", s);
            conds.push(
                "(title_romaji LIKE ? OR title_english LIKE ? OR title_native LIKE ? \
                  OR title_user_preferred LIKE ? OR title_russian LIKE ? OR title_ru_machine LIKE ? OR synonyms LIKE ?)".into(),
            );
            for _ in 0..7 {
                params.push(Box::new(like.clone()));
            }
        }
    }

    if let Some(f) = q.format.as_ref() {
        if !f.is_empty() {
            conds.push("format = ?".into());
            params.push(Box::new(f.clone()));
        }
    }

    if let Some(s) = q.status.as_ref() {
        if !s.is_empty() {
            conds.push("status = ?".into());
            params.push(Box::new(s.clone()));
        }
    }

    if let Some(s) = q.season.as_ref() {
        if !s.is_empty() {
            conds.push("season = ?".into());
            params.push(Box::new(s.clone()));
        }
    }

    if let Some(yf) = q.year_from {
        conds.push("(start_year IS NULL OR start_year >= ?)".into());
        params.push(Box::new(yf));
    }
    if let Some(yt) = q.year_to {
        conds.push("(start_year IS NULL OR start_year <= ?)".into());
        params.push(Box::new(yt));
    }

    if let Some(sf) = q.score_from {
        conds.push("COALESCE(average_score, 0) >= ?".into());
        params.push(Box::new(sf));
    }
    if let Some(st) = q.score_to {
        conds.push("COALESCE(average_score, 0) <= ?".into());
        params.push(Box::new(st));
    }

    if let Some(g) = q.genre.as_ref() {
        if !g.is_empty() {
            conds.push("(genres_json LIKE ? OR jikan_genres_json LIKE ?)".into());
            let needle = format!("%\"{}\"%", g);
            params.push(Box::new(needle.clone()));
            params.push(Box::new(needle));
        }
    }

    if let Some(c) = q.country.as_ref() {
        if !c.is_empty() {
            conds.push("country_of_origin = ?".into());
            params.push(Box::new(c.clone()));
        }
    }

    match q.adult.as_deref() {
        Some("no") => conds.push("COALESCE(is_adult, 0) = 0".into()),
        Some("only") => conds.push("COALESCE(is_adult, 0) = 1".into()),
        _ => {}
    }

    match q.licensed.as_deref() {
        Some("yes") => conds.push("COALESCE(is_licensed, 0) = 1".into()),
        Some("no") => conds.push("COALESCE(is_licensed, 0) = 0".into()),
        _ => {}
    }

    if q.has_ru.as_deref() == Some("yes") {
        conds.push("(COALESCE(title_russian, '') != '' OR COALESCE(title_ru_machine, '') != '')".into());
    }

    let sql = if conds.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conds.join(" AND "))
    };
    (sql, params)
}

fn order_clause(sort: &str) -> &'static str {
    match sort {
        "score" => "ORDER BY average_score DESC NULLS LAST, popularity DESC",
        "score_asc" => "ORDER BY average_score ASC NULLS LAST, popularity DESC",
        "mal_score" => "ORDER BY mal_score DESC NULLS LAST",
        "start_date" => "ORDER BY start_year DESC NULLS LAST, start_month DESC NULLS LAST, start_day DESC NULLS LAST",
        "start_date_asc" => "ORDER BY start_year ASC NULLS LAST",
        "title" => "ORDER BY COALESCE(title_romaji, title_english) ASC",
        "title_desc" => "ORDER BY COALESCE(title_romaji, title_english) DESC",
        "title_ru" => "ORDER BY COALESCE(NULLIF(title_russian, ''), title_ru_machine, title_romaji) ASC",
        "favourites" => "ORDER BY favourites DESC NULLS LAST",
        "trending" => "ORDER BY trending DESC NULLS LAST",
        "episodes" => "ORDER BY episodes DESC NULLS LAST",
        "id_desc" => "ORDER BY anilist_id DESC",
        "id" => "ORDER BY anilist_id ASC",
        _ => "ORDER BY popularity DESC NULLS LAST, anilist_id ASC",
    }
}

pub async fn api_list(query: web::Query<PageQuery>) -> impl Responder {
    let q = query.into_inner();
    let page = q.page.unwrap_or(1).max(1);
    let per_page: i64 = 50;
    let offset: i64 = (page as i64 - 1) * per_page;
    let sort = q.sort.clone().unwrap_or_else(|| "popularity".into());

    let result = web::block(move || -> Result<Vec<AnimeListItem>, rusqlite::Error> {
        let conn = db::open()?;
        db::ensure_schema(&conn)?;

        let (where_sql, params) = build_filters(&q);
        let order_sql = order_clause(&sort);

        let sql = format!(
            "SELECT anilist_id,
                    COALESCE(title_romaji, title_english, title_native, '?'),
                    title_russian,
                    COALESCE(cover_large, cover_medium, cover_extra_large, ''),
                    COALESCE(average_score, 0),
                    start_year, format, status, episodes,
                    title_ru_machine, title_english
             FROM anime {}
             {} LIMIT ? OFFSET ?",
            where_sql, order_sql
        );

        let mut stmt = conn.prepare(&sql)?;

        let mut all_params: Vec<Box<dyn rusqlite::ToSql>> = params;
        all_params.push(Box::new(per_page));
        all_params.push(Box::new(offset));

        let param_refs: Vec<&dyn rusqlite::ToSql> = all_params.iter().map(|b| b.as_ref()).collect();

        let mapped = stmt.query_map(param_refs.as_slice(), |r| {
            Ok(AnimeListItem {
                id: r.get(0)?,
                title: r.get(1)?,
                title_russian: r.get(2)?,
                cover: r.get(3)?,
                score: r.get(4)?,
                year: r.get(5)?,
                format: r.get(6)?,
                status: r.get(7)?,
                episodes: r.get(8)?,
                title_ru_machine: r.get(9)?,
                title_english: r.get(10)?,
            })
        })?;

        let collected: Vec<AnimeListItem> = mapped.filter_map(|x| x.ok()).collect();
        Ok(collected)
    })
    .await;

    match result {
        Ok(Ok(list)) => HttpResponse::Ok().json(list),
        Ok(Err(e)) => {
            eprintln!("[API ERR] {}", e);
            HttpResponse::InternalServerError().body(format!("DB: {}", e))
        }
        Err(e) => {
            eprintln!("[API ERR] {}", e);
            HttpResponse::InternalServerError().body("Internal")
        }
    }
}

pub async fn api_count(query: web::Query<PageQuery>) -> impl Responder {
    let q = query.into_inner();

    let result = web::block(move || -> Result<i64, rusqlite::Error> {
        let conn = db::open()?;
        db::ensure_schema(&conn)?;
        let (where_sql, params) = build_filters(&q);
        let sql = format!("SELECT COUNT(*) FROM anime {}", where_sql);

        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        stmt.query_row(param_refs.as_slice(), |r| r.get(0))
    })
    .await;

    match result {
        Ok(Ok(n)) => HttpResponse::Ok().json(serde_json::json!({ "count": n })),
        _ => HttpResponse::InternalServerError().body("DB error"),
    }
}

pub async fn api_filters() -> impl Responder {
    let result = web::block(move || -> Result<Value, rusqlite::Error> {
        let conn = db::open()?;
        db::ensure_schema(&conn)?;

        let mut stmt = conn.prepare(
            "SELECT DISTINCT format FROM anime WHERE format IS NOT NULL AND format != '' ORDER BY format",
        )?;
        let formats: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .filter_map(|x| x.ok())
            .collect();
        drop(stmt);

        let mut stmt = conn.prepare(
            "SELECT DISTINCT status FROM anime WHERE status IS NOT NULL AND status != '' ORDER BY status",
        )?;
        let statuses: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .filter_map(|x| x.ok())
            .collect();
        drop(stmt);

        let mut stmt = conn.prepare(
            "SELECT DISTINCT season FROM anime WHERE season IS NOT NULL AND season != '' ORDER BY season",
        )?;
        let seasons: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .filter_map(|x| x.ok())
            .collect();
        drop(stmt);

        let mut stmt = conn.prepare(
            "SELECT DISTINCT country_of_origin FROM anime WHERE country_of_origin IS NOT NULL AND country_of_origin != '' ORDER BY country_of_origin",
        )?;
        let countries: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .filter_map(|x| x.ok())
            .collect();
        drop(stmt);

        let (y_min, y_max): (Option<i64>, Option<i64>) = conn.query_row(
            "SELECT MIN(start_year), MAX(start_year) FROM anime WHERE start_year IS NOT NULL",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;

        let mut stmt = conn.prepare("SELECT genres_json FROM anime WHERE genres_json IS NOT NULL")?;
        let mut genre_set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for row in rows.flatten() {
            if let Ok(arr) = serde_json::from_str::<Vec<String>>(&row) {
                for g in arr {
                    genre_set.insert(g);
                }
            }
        }
        drop(stmt);

        let genres: Vec<String> = genre_set.into_iter().collect();

        Ok(serde_json::json!({
            "formats": formats,
            "statuses": statuses,
            "seasons": seasons,
            "countries": countries,
            "genres": genres,
            "year_min": y_min,
            "year_max": y_max,
        }))
    })
    .await;

    match result {
        Ok(Ok(v)) => HttpResponse::Ok().json(v),
        Ok(Err(e)) => {
            eprintln!("[API ERR] {}", e);
            HttpResponse::InternalServerError().body(format!("DB: {}", e))
        }
        Err(e) => {
            eprintln!("[API ERR] {}", e);
            HttpResponse::InternalServerError().body("Internal")
        }
    }
}

pub async fn api_detail(path: web::Path<i64>) -> impl Responder {
    let id = path.into_inner();

    let result = web::block(move || -> Result<Option<Value>, rusqlite::Error> {
        let conn = db::open()?;
        db::ensure_schema(&conn)?;

        let row = conn
            .query_row(
                "SELECT * FROM anime WHERE anilist_id = ?1",
                [id],
                |r| {
                    let cols = r.as_ref().column_count();
                    let mut obj = serde_json::Map::new();
                    for i in 0..cols {
                        let name = r.as_ref().column_name(i).unwrap_or("?").to_string();
                        let v: Value = match r.get_ref(i) {
                            Ok(rusqlite::types::ValueRef::Null) => Value::Null,
                            Ok(rusqlite::types::ValueRef::Integer(n)) => Value::Number(n.into()),
                            Ok(rusqlite::types::ValueRef::Real(f)) => {
                                serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null)
                            }
                            Ok(rusqlite::types::ValueRef::Text(t)) => {
                                let s = String::from_utf8_lossy(t).to_string();
                                // Попытка распарсить JSON-поля
                                if name.ends_with("_json") {
                                    serde_json::from_str(&s).unwrap_or(Value::String(s))
                                } else {
                                    Value::String(s)
                                }
                            }
                            _ => Value::Null,
                        };
                        obj.insert(name, v);
                    }
                    Ok(Value::Object(obj))
                },
            )
            .optional()?;

        Ok(row)
    })
    .await;

    match result {
        Ok(Ok(Some(v))) => HttpResponse::Ok().json(v),
        Ok(Ok(None)) => HttpResponse::NotFound().body("Not found"),
        _ => HttpResponse::InternalServerError().body("DB error"),
    }
}

pub async fn index_html() -> impl Responder {
    match std::fs::read_to_string("frontend/index.html") {
        Ok(html) => HttpResponse::Ok().content_type("text/html; charset=utf-8").body(html),
        Err(e) => HttpResponse::InternalServerError()
            .content_type("text/plain; charset=utf-8")
            .body(format!("Не найден frontend/index.html: {}", e)),
    }
}

pub async fn detail_html() -> impl Responder {
    match std::fs::read_to_string("frontend/detail.html") {
        Ok(html) => HttpResponse::Ok().content_type("text/html; charset=utf-8").body(html),
        Err(e) => HttpResponse::InternalServerError()
            .content_type("text/plain; charset=utf-8")
            .body(format!("Не найден frontend/detail.html: {}", e)),
    }
}

pub async fn api_progress() -> impl Responder {
    let result = web::block(move || -> Result<Value, rusqlite::Error> {
        let conn = db::open()?;
        let mut stmt = conn.prepare(
            "SELECT source, task, last_page, total_saved, finished, updated_at FROM sync_state ORDER BY source, task",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(serde_json::json!({
                "source": r.get::<_, String>(0)?,
                "task": r.get::<_, String>(1)?,
                "last_page": r.get::<_, i64>(2)?,
                "total_saved": r.get::<_, i64>(3)?,
                "finished": r.get::<_, i64>(4)? == 1,
                "updated_at": r.get::<_, Option<i64>>(5)?,
            }))
        })?;
        let list: Vec<Value> = rows.filter_map(|x| x.ok()).collect();
        Ok(Value::Array(list))
    })
    .await;

    match result {
        Ok(Ok(v)) => HttpResponse::Ok().json(v),
        _ => HttpResponse::InternalServerError().body("DB error"),
    }
}
// ============================================================
// Справочник жанров
// ============================================================

pub async fn api_genres() -> impl Responder {
    let result = web::block(move || -> Result<Value, rusqlite::Error> {
        let conn = db::open()?;
        db::ensure_schema(&conn)?;

        let mut stmt = conn.prepare(
            "SELECT id, name_en, name_ru, category, auto_created,
                    (SELECT COUNT(*) FROM anime_genres WHERE genre_id = genres_dict.id) as count
             FROM genres_dict
             WHERE count > 0
             ORDER BY category, name_en",
        )?;

        let rows = stmt.query_map([], |r| {
            Ok(serde_json::json!({
                "id": r.get::<_, i64>(0)?,
                "name_en": r.get::<_, String>(1)?,
                "name_ru": r.get::<_, Option<String>>(2)?,
                "category": r.get::<_, Option<String>>(3)?,
                "auto_created": r.get::<_, i64>(4)? == 1,
                "count": r.get::<_, i64>(5)?,
            }))
        })?;

        let list: Vec<Value> = rows.filter_map(|x| x.ok()).collect();
        Ok(Value::Array(list))
    })
    .await;

    match result {
        Ok(Ok(v)) => HttpResponse::Ok().json(v),
        _ => HttpResponse::InternalServerError().body("DB error"),
    }
}

pub async fn api_genre_anime(
    path: web::Path<i64>,
    query: web::Query<PageQuery>,
) -> impl Responder {
    let genre_id = path.into_inner();
    let q = query.into_inner();
    let page = q.page.unwrap_or(1).max(1);
    let per_page: i64 = 50;
    let offset: i64 = (page as i64 - 1) * per_page;
    let sort = q.sort.clone().unwrap_or_else(|| "popularity".into());

    let result = web::block(move || -> Result<Vec<AnimeListItem>, rusqlite::Error> {
        let conn = db::open()?;
        let order_sql = order_clause(&sort);

        let sql = format!(
            "SELECT a.anilist_id,
                    COALESCE(a.title_romaji, a.title_english, a.title_native, '?'),
                    a.title_russian,
                    COALESCE(a.cover_large, a.cover_medium, a.cover_extra_large, ''),
                    COALESCE(a.average_score, 0),
                    a.start_year, a.format, a.status, a.episodes,
                    a.title_ru_machine, a.title_english
             FROM anime a
             JOIN anime_genres ag ON ag.anilist_id = a.anilist_id
             WHERE ag.genre_id = ?1
             {} LIMIT ?2 OFFSET ?3",
            order_sql
        );

        let mut stmt = conn.prepare(&sql)?;
        let mapped = stmt.query_map(rusqlite::params![genre_id, per_page, offset], |r| {
            Ok(AnimeListItem {
                id: r.get(0)?,
                title: r.get(1)?,
                title_russian: r.get(2)?,
                cover: r.get(3)?,
                score: r.get(4)?,
                year: r.get(5)?,
                format: r.get(6)?,
                status: r.get(7)?,
                episodes: r.get(8)?,
                title_ru_machine: r.get(9)?,
                title_english: r.get(10)?,
            })
        })?;

        let collected: Vec<AnimeListItem> = mapped.filter_map(|x| x.ok()).collect();
        Ok(collected)
    })
    .await;

    match result {
        Ok(Ok(list)) => HttpResponse::Ok().json(list),
        Ok(Err(e)) => {
            eprintln!("[API ERR] {}", e);
            HttpResponse::InternalServerError().body(format!("DB: {}", e))
        }
        Err(_) => HttpResponse::InternalServerError().body("Internal"),
    }
}