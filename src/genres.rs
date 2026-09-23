use rusqlite::Connection;
use serde_json::Value;

/// Нормализация названия жанра: убирает лишние пробелы, приводит к нижнему регистру.
fn norm(s: &str) -> String {
    s.trim().to_lowercase()
}

/// Находит ID жанра в справочнике по английскому названию.
/// Если нет — создаёт с auto_created=1 и name_ru=NULL.
fn find_or_create_genre(
    conn: &Connection,
    name_en: &str,
    category: &str,
) -> Result<i64, rusqlite::Error> {
    // Сначала точное совпадение (регистронезависимо)
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM genres_dict WHERE LOWER(name_en) = LOWER(?1) LIMIT 1",
            [name_en],
            |r| r.get(0),
        )
        .ok();

    if let Some(id) = existing {
        return Ok(id);
    }

    // Нет — создаём
    conn.execute(
        "INSERT INTO genres_dict (name_en, name_ru, category, auto_created)
         VALUES (?1, NULL, ?2, 1)",
        rusqlite::params![name_en, category],
    )?;

    Ok(conn.last_insert_rowid())
}

/// Собирает все жанры/теги из JSON-полей одной аниме-записи и кладёт в anime_genres.
pub fn match_genres_for_anime(
    conn: &Connection,
    anilist_id: i64,
) -> Result<usize, rusqlite::Error> {
    // Читаем все JSON-поля
    let row: (Option<String>, Option<String>, Option<String>, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT genres_json, tags_json, jikan_genres_json, jikan_themes_json, jikan_demographics_json
             FROM anime WHERE anilist_id = ?1",
            [anilist_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;

    let (genres_json, tags_json, jik_genres, jik_themes, jik_demo) = row;

    let mut inserted = 0usize;

    // --- AniList genres (массив строк) ---
    if let Some(s) = genres_json {
        if let Ok(arr) = serde_json::from_str::<Vec<String>>(&s) {
            for name in arr {
                let name = name.trim();
                if name.is_empty() { continue; }
                let gid = find_or_create_genre(conn, name, "genre")?;
                if insert_link(conn, anilist_id, gid, "anilist").is_ok() {
                    inserted += 1;
                }
            }
        }
    }

    // --- AniList tags (массив объектов {name, rank, isMediaSpoiler}) ---
    if let Some(s) = tags_json {
        if let Ok(arr) = serde_json::from_str::<Vec<Value>>(&s) {
            for obj in arr {
                if let Some(name) = obj.get("name").and_then(|x| x.as_str()) {
                    let name = name.trim();
                    if name.is_empty() { continue; }
                    let gid = find_or_create_genre(conn, name, "tag")?;
                    if insert_link(conn, anilist_id, gid, "tag").is_ok() {
                        inserted += 1;
                    }
                }
            }
        }
    }

    // --- Jikan genres (массив {name}) ---
    if let Some(s) = jik_genres {
        if let Ok(arr) = serde_json::from_str::<Vec<Value>>(&s) {
            for obj in arr {
                if let Some(name) = obj.get("name").and_then(|x| x.as_str()) {
                    let name = name.trim();
                    if name.is_empty() { continue; }
                    let gid = find_or_create_genre(conn, name, "genre")?;
                    if insert_link(conn, anilist_id, gid, "jikan").is_ok() {
                        inserted += 1;
                    }
                }
            }
        }
    }

    // --- Jikan themes ---
    if let Some(s) = jik_themes {
        if let Ok(arr) = serde_json::from_str::<Vec<Value>>(&s) {
            for obj in arr {
                if let Some(name) = obj.get("name").and_then(|x| x.as_str()) {
                    let name = name.trim();
                    if name.is_empty() { continue; }
                    let gid = find_or_create_genre(conn, name, "theme")?;
                    if insert_link(conn, anilist_id, gid, "jikan_theme").is_ok() {
                        inserted += 1;
                    }
                }
            }
        }
    }

    // --- Jikan demographics ---
    if let Some(s) = jik_demo {
        if let Ok(arr) = serde_json::from_str::<Vec<Value>>(&s) {
            for obj in arr {
                if let Some(name) = obj.get("name").and_then(|x| x.as_str()) {
                    let name = name.trim();
                    if name.is_empty() { continue; }
                    let gid = find_or_create_genre(conn, name, "demographic")?;
                    if insert_link(conn, anilist_id, gid, "jikan_demo").is_ok() {
                        inserted += 1;
                    }
                }
            }
        }
    }

    // Отмечаем, что жанры сопоставлены
    conn.execute(
        "UPDATE anime SET genres_matched_at = strftime('%s','now') WHERE anilist_id = ?1",
        [anilist_id],
    )?;

    Ok(inserted)
}

fn insert_link(
    conn: &Connection,
    anilist_id: i64,
    genre_id: i64,
    source: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR IGNORE INTO anime_genres (anilist_id, genre_id, source) VALUES (?1, ?2, ?3)",
        rusqlite::params![anilist_id, genre_id, source],
    )?;
    Ok(())
}

/// Проходит по всей БД и сопоставляет жанры для тех, у кого не сделано.
pub async fn match_all_genres() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n========== Сопоставление жанров ==========");

    let conn = crate::db::open()?;
    crate::db::ensure_schema(&conn)?;

    let mut total_done = 0i64;

    loop {
        let batch: Vec<i64> = {
            let mut stmt = conn.prepare(
                "SELECT anilist_id FROM anime
                 WHERE genres_matched_at IS NULL
                 ORDER BY anilist_id ASC
                 LIMIT 500",
            )?;
            let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
            rows.filter_map(|x| x.ok()).collect()
        };

        if batch.is_empty() {
            println!("[Genres] всё сопоставлено. Всего: {}", total_done);
            break;
        }

        for id in &batch {
            match match_genres_for_anime(&conn, *id) {
                Ok(_) => total_done += 1,
                Err(e) => eprintln!("[Genres] id={}: {}", id, e),
            }
        }

        println!("[Genres] обработано: {}", total_done);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    Ok(())
}