use crate::db;
use crate::sources::{anilist, jikan, shikimori};
use rusqlite::Connection;
use std::time::Duration;

// ============================================================
// AniList — загрузка базовой информации
// ============================================================

pub async fn load_anilist() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n========== AniList ==========");

    let client = reqwest::Client::builder()
        .user_agent("AnimeLoader/2.0")
        .timeout(Duration::from_secs(60))
        .build()?;

    let conn = db::open()?;
    db::ensure_schema(&conn)?;

    let sorts = [
        "POPULARITY_DESC",
        "SCORE_DESC",
        "START_DATE_DESC",
        "TRENDING_DESC",
        "FAVOURITES_DESC",
        "ID_DESC",
        "ID",
        "TITLE_ROMAJI",
    ];

    let pages_per_sort: u32 = 100;
    let per_page: u32 = 50;

    for sort in sorts {
        let task = format!("sort:{}", sort);

        let (last_page, finished) = db::get_checkpoint(&conn, "anilist", &task);
        if finished {
            println!("[AniList][{}] уже завершён, пропускаю", sort);
            continue;
        }
        let start_page = (last_page + 1) as u32;
        if start_page > 1 {
            println!("[AniList][{}] продолжаю с страницы {}", sort, start_page);
        }

        let mut consecutive_errors = 0;
        let mut total_for_task: i64 = conn
            .query_row(
                "SELECT COALESCE(total_saved, 0) FROM sync_state WHERE source='anilist' AND task=?1",
                [&task],
                |r| r.get(0),
            )
            .unwrap_or(0);

        for page in start_page..=pages_per_sort {
            match anilist::fetch_page(&client, page, per_page, sort).await {
                Ok((media_list, has_next)) => {
                    consecutive_errors = 0;

                    if media_list.is_empty() {
                        db::save_checkpoint(&conn, "anilist", &task, page as i64, total_for_task, true)?;
                        break;
                    }

                    let mut ok = 0;
                    for m in &media_list {
                        if upsert_anilist(&conn, m).is_ok() {
                            ok += 1;
                        }
                    }
                    total_for_task += ok as i64;

                    db::save_checkpoint(
                        &conn,
                        "anilist",
                        &task,
                        page as i64,
                        total_for_task,
                        !has_next,
                    )?;

                    let total_db: i64 = conn
                        .query_row("SELECT COUNT(*) FROM anime", [], |r| r.get(0))
                        .unwrap_or(0);
                    println!(
                        "[AniList][{}] стр. {}: +{}; всего в БД: {}",
                        sort, page, ok, total_db
                    );

                    if !has_next {
                        break;
                    }
                }
                Err(e) => {
                    consecutive_errors += 1;
                    eprintln!("[AniList][{}] стр. {}: {}", sort, page, e);
                    if consecutive_errors >= 5 {
                        eprintln!("[AniList][{}] 5 ошибок, переход", sort);
                        break;
                    }
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
            }

            tokio::time::sleep(Duration::from_millis(750)).await;
        }
    }

    Ok(())
}

fn upsert_anilist(conn: &Connection, m: &anilist::Media) -> Result<(), rusqlite::Error> {
    use serde_json::to_string;

    let t = &m.title;

    let (cover_xl, cover_l, cover_m, cover_color) = match &m.cover_image {
        Some(c) => (
            c.extra_large.clone(),
            c.large.clone(),
            c.medium.clone(),
            c.color.clone(),
        ),
        None => (None, None, None, None),
    };

    let (tr_id, tr_site, tr_thumb) = match &m.trailer {
        Some(t) => (t.id.clone(), t.site.clone(), t.thumbnail.clone()),
        None => (None, None, None),
    };

    let sd = m.start_date.as_ref();
    let ed = m.end_date.as_ref();

    let synonyms_json = m.synonyms.as_ref().and_then(|x| to_string(x).ok());
    let genres_json = m.genres.as_ref().and_then(|x| to_string(x).ok());
    let tags_json = m.tags.as_ref().and_then(|x| to_string(x).ok());

    let pieces = anilist::media_to_json_pieces(m);

    let studios_json = pieces.get("studios").and_then(|v| to_string(v).ok());
    let relations_json = pieces.get("relations").and_then(|v| to_string(v).ok());
    let external_links_json = pieces.get("external_links").and_then(|v| to_string(v).ok());
    let streaming_json = pieces.get("streaming_episodes").and_then(|v| to_string(v).ok());
    let recs_json = pieces.get("recommendations").and_then(|v| to_string(v).ok());

    let bool_to_int = |b: Option<bool>| b.map(|x| if x { 1 } else { 0 });

    conn.execute(
        r#"INSERT INTO anime (
            anilist_id, mal_id,
            title_romaji, title_english, title_native, title_user_preferred, synonyms,
            format, status, description, duration, episodes, chapters, volumes,
            country_of_origin, is_adult, is_licensed,
            start_year, start_month, start_day,
            end_year, end_month, end_day,
            season, season_year,
            average_score, mean_score, popularity, favourites, trending,
            cover_extra_large, cover_large, cover_medium, cover_color, banner_image,
            trailer_id, trailer_site, trailer_thumbnail,
            genres_json, tags_json, studios_json,
            relations_json, external_links_json, streaming_episodes_json,
            recommendations_json,
            anilist_synced_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
            ?8, ?9, ?10, ?11, ?12, ?13, ?14,
            ?15, ?16, ?17,
            ?18, ?19, ?20, ?21, ?22, ?23,
            ?24, ?25,
            ?26, ?27, ?28, ?29, ?30,
            ?31, ?32, ?33, ?34, ?35,
            ?36, ?37, ?38,
            ?39, ?40, ?41,
            ?42, ?43, ?44,
            ?45,
            strftime('%s','now')
        )
        ON CONFLICT(anilist_id) DO UPDATE SET
            mal_id = COALESCE(excluded.mal_id, anime.mal_id),
            title_romaji = COALESCE(excluded.title_romaji, anime.title_romaji),
            title_english = COALESCE(excluded.title_english, anime.title_english),
            title_native = COALESCE(excluded.title_native, anime.title_native),
            title_user_preferred = COALESCE(excluded.title_user_preferred, anime.title_user_preferred),
            synonyms = COALESCE(excluded.synonyms, anime.synonyms),
            format = COALESCE(excluded.format, anime.format),
            status = COALESCE(excluded.status, anime.status),
            description = COALESCE(excluded.description, anime.description),
            duration = COALESCE(excluded.duration, anime.duration),
            episodes = COALESCE(excluded.episodes, anime.episodes),
            country_of_origin = COALESCE(excluded.country_of_origin, anime.country_of_origin),
            is_adult = COALESCE(excluded.is_adult, anime.is_adult),
            is_licensed = COALESCE(excluded.is_licensed, anime.is_licensed),
            start_year = COALESCE(excluded.start_year, anime.start_year),
            season = COALESCE(excluded.season, anime.season),
            season_year = COALESCE(excluded.season_year, anime.season_year),
            average_score = COALESCE(excluded.average_score, anime.average_score),
            mean_score = COALESCE(excluded.mean_score, anime.mean_score),
            popularity = COALESCE(excluded.popularity, anime.popularity),
            favourites = COALESCE(excluded.favourites, anime.favourites),
            trending = COALESCE(excluded.trending, anime.trending),
            cover_extra_large = COALESCE(excluded.cover_extra_large, anime.cover_extra_large),
            cover_large = COALESCE(excluded.cover_large, anime.cover_large),
            cover_medium = COALESCE(excluded.cover_medium, anime.cover_medium),
            cover_color = COALESCE(excluded.cover_color, anime.cover_color),
            banner_image = COALESCE(excluded.banner_image, anime.banner_image),
            trailer_id = COALESCE(excluded.trailer_id, anime.trailer_id),
            trailer_site = COALESCE(excluded.trailer_site, anime.trailer_site),
            trailer_thumbnail = COALESCE(excluded.trailer_thumbnail, anime.trailer_thumbnail),
            genres_json = COALESCE(excluded.genres_json, anime.genres_json),
            tags_json = COALESCE(excluded.tags_json, anime.tags_json),
            studios_json = COALESCE(excluded.studios_json, anime.studios_json),
            relations_json = COALESCE(excluded.relations_json, anime.relations_json),
            external_links_json = COALESCE(excluded.external_links_json, anime.external_links_json),
            streaming_episodes_json = COALESCE(excluded.streaming_episodes_json, anime.streaming_episodes_json),
            recommendations_json = COALESCE(excluded.recommendations_json, anime.recommendations_json),
            anilist_synced_at = strftime('%s','now')"#,
        rusqlite::params![
            m.id,
            m.id_mal,
            t.romaji,
            t.english,
            t.native,
            t.user_preferred,
            synonyms_json,
            m.format,
            m.status,
            m.description,
            m.duration,
            m.episodes,
            m.chapters,
            m.volumes,
            m.country_of_origin,
            bool_to_int(m.is_adult),
            bool_to_int(m.is_licensed),
            sd.and_then(|d| d.year),
            sd.and_then(|d| d.month),
            sd.and_then(|d| d.day),
            ed.and_then(|d| d.year),
            ed.and_then(|d| d.month),
            ed.and_then(|d| d.day),
            m.season,
            m.season_year,
            m.average_score,
            m.mean_score,
            m.popularity,
            m.favourites,
            m.trending,
            cover_xl,
            cover_l,
            cover_m,
            cover_color,
            m.banner_image,
            tr_id,
            tr_site,
            tr_thumb,
            genres_json,
            tags_json,
            studios_json,
            relations_json,
            external_links_json,
            streaming_json,
            recs_json,
        ],
    )?;

    Ok(())
}

// ============================================================
// Jikan — оставлен для совместимости, НЕ ЗАПУСКАЕТСЯ
// ============================================================

pub async fn load_jikan() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n========== Jikan (MAL) ==========");

    let client = reqwest::Client::builder()
        .user_agent("AnimeLoader/2.0")
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(30))
        .build()?;

    let conn = db::open()?;
    db::ensure_schema(&conn)?;

    let (last_page, finished) = db::get_checkpoint(&conn, "jikan", "top");

    if !finished {
        let mut consecutive_errors = 0;
        let mut total_saved: i64 = conn
            .query_row(
                "SELECT COALESCE(total_saved,0) FROM sync_state WHERE source='jikan' AND task='top'",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);

        for page in ((last_page + 1).max(1) as u32)..=200 {
            match jikan::fetch_top_page(&client, page).await {
                Ok((items, has_next)) => {
                    consecutive_errors = 0;
                    let mut ok = 0;
                    for item in &items {
                        if upsert_jikan_basic(&conn, item).is_ok() {
                            ok += 1;
                        }
                    }
                    total_saved += ok;
                    db::save_checkpoint(&conn, "jikan", "top", page as i64, total_saved, !has_next)?;

                    println!("[Jikan][top] стр. {}: +{}; всего: {}", page, ok, total_saved);

                    if !has_next {
                        break;
                    }
                }
                Err(e) => {
                    consecutive_errors += 1;
                    eprintln!("[Jikan][top] стр. {}: {}", page, e);
                    if consecutive_errors >= 5 {
                        break;
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
    } else {
        println!("[Jikan][top] уже завершён");
    }

    Ok(())
}

fn upsert_jikan_basic(conn: &Connection, a: &jikan::JikanAnime) -> Result<(), rusqlite::Error> {
    let existing: Option<i64> = conn
        .query_row(
            "SELECT anilist_id FROM anime WHERE mal_id = ?1",
            [a.mal_id],
            |r| r.get(0),
        )
        .ok();

    let year = a.year;
    let score_100 = a.score.map(|s| (s * 10.0).round() as i64);

    let jikan_genres = a.genres.as_ref().map(|g| serde_json::to_string(g).unwrap_or_default());
    let jikan_themes = a.themes.as_ref().map(|g| serde_json::to_string(g).unwrap_or_default());
    let jikan_demographics = a.demographics.as_ref().map(|g| serde_json::to_string(g).unwrap_or_default());
    let jikan_studios = a.studios.as_ref().map(|g| serde_json::to_string(g).unwrap_or_default());
    let jikan_producers = a.producers.as_ref().map(|g| serde_json::to_string(g).unwrap_or_default());
    let jikan_licensors = a.licensors.as_ref().map(|g| serde_json::to_string(g).unwrap_or_default());

    if let Some(existing_id) = existing {
        conn.execute(
            "UPDATE anime SET
                mal_score = ?1, mal_rank = ?2, mal_members = ?3,
                jikan_genres_json = ?4, jikan_themes_json = ?5, jikan_demographics_json = ?6,
                jikan_studios_json = ?7, jikan_producers_json = ?8, jikan_licensors_json = ?9,
                jikan_synced_at = strftime('%s','now')
             WHERE anilist_id = ?10",
            rusqlite::params![
                a.score, a.rank, a.members,
                jikan_genres, jikan_themes, jikan_demographics,
                jikan_studios, jikan_producers, jikan_licensors,
                existing_id,
            ],
        )?;
    } else {
        let fake_id = -a.mal_id;
        conn.execute(
            "INSERT OR IGNORE INTO anime (
                anilist_id, mal_id,
                title_romaji, title_english, title_native,
                format, status, description, episodes,
                start_year, season,
                average_score, popularity, favourites,
                cover_large, trailer_id, trailer_site,
                jikan_genres_json, jikan_themes_json, jikan_demographics_json,
                jikan_studios_json, jikan_producers_json, jikan_licensors_json,
                jikan_synced_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5,
                ?6, ?7, ?8, ?9,
                ?10, ?11,
                ?12, ?13, ?14,
                ?15, ?16, ?17,
                ?18, ?19, ?20,
                ?21, ?22, ?23,
                strftime('%s','now')
            )",
            rusqlite::params![
                fake_id, a.mal_id,
                a.title, a.title_english, a.title_japanese,
                a.format, a.status, a.synopsis, a.episodes,
                year, a.season,
                score_100, a.popularity, a.favorites,
                a.images.as_ref().and_then(|v| v.pointer("/jpg/large_image_url")).and_then(|x| x.as_str()),
                a.trailer.as_ref().and_then(|v| v.get("youtube_id")).and_then(|x| x.as_str()),
                if a.trailer.is_some() { Some("youtube") } else { None },
                jikan_genres, jikan_themes, jikan_demographics,
                jikan_studios, jikan_producers, jikan_licensors,
            ],
        )?;
    }

    Ok(())
}

fn enrich_jikan_full(
    conn: &Connection,
    mal_id: i64,
    v: &serde_json::Value,
) -> Result<(), rusqlite::Error> {
    let staff = v.get("staff").and_then(|x| serde_json::to_string(x).ok());
    let characters = v.get("characters").and_then(|x| serde_json::to_string(x).ok());

    conn.execute(
        "UPDATE anime SET
            jikan_staff_json = ?1,
            jikan_characters_json = ?2,
            jikan_synced_at = strftime('%s','now')
         WHERE mal_id = ?3",
        rusqlite::params![staff, characters, mal_id],
    )?;

    Ok(())
}

// ============================================================
// Shikimori — ТОЛЬКО русские названия. Без русского = пропуск.
// ============================================================

pub async fn load_shikimori() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n========== Shikimori (RU-only) ==========");

    let client = reqwest::Client::builder()
        .user_agent("AnimeDatabase/1.0 (https://github.com/teivrim/anime-db)")
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(20))
        .build()?;

    // === ПИНГ-ТЕСТ ===
    println!("[Shikimori] Пинг-тест...");
    match shikimori::ping_test(&client).await {
        Ok(Some(russian)) => {
            println!("[Shikimori] ✓ Пинг OK. Пример: russian = \"{}\"", russian);
        }
        Ok(None) => {
            println!("[Shikimori] ⚠ Пинг OK, но russian пустой");
        }
        Err(e) => {
            eprintln!("[Shikimori] ✗ Пинг провалился: {}", e);
            eprintln!("[Shikimori] Пропускаю.");
            return Ok(());
        }
    }

    let conn = db::open()?;
    db::ensure_schema(&conn)?;

    let (last_page, finished) = db::get_checkpoint(&conn, "shikimori", "top");
    if finished {
        println!("[Shikimori] уже завершён");
        return Ok(());
    }
    let start_page = (last_page + 1).max(1) as u32;

    let mut consecutive_errors = 0;
    let mut total_saved: i64 = conn
        .query_row(
            "SELECT COALESCE(total_saved,0) FROM sync_state WHERE source='shikimori' AND task='top'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let mut total_skipped_no_ru: i64 = 0;

    for page in start_page..=500 {
        match shikimori::fetch_page(&client, page).await {
            Ok((items, has_next)) => {
                consecutive_errors = 0;
                let mut ok = 0;
                let mut skipped = 0;

                for item in &items {
                    // Проверяем, есть ли русское название
                    let has_ru = item
                        .russian
                        .as_deref()
                        .map(|s| !s.trim().is_empty())
                        .unwrap_or(false);

                    if !has_ru {
                        skipped += 1;
                        continue; // ← без русского пропускаем
                    }

                    if upsert_shikimori(&conn, item).is_ok() {
                        ok += 1;
                    }
                }
                total_saved += ok;
                total_skipped_no_ru += skipped;

                db::save_checkpoint(
                    &conn,
                    "shikimori",
                    "top",
                    page as i64,
                    total_saved,
                    !has_next,
                )?;

                let db_ru_count: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM anime WHERE title_russian IS NOT NULL AND title_russian != ''",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap_or(0);

                println!(
                    "[Shikimori] стр. {}: +{} (пропущено без RU: {}); всего RU в БД: {}",
                    page, ok, skipped, db_ru_count
                );

                if !has_next {
                    break;
                }
            }
            Err(e) => {
                consecutive_errors += 1;
                eprintln!("[Shikimori] стр. {}: {}", page, e);
                if consecutive_errors >= 5 {
                    eprintln!("[Shikimori] 5 ошибок подряд, выходим");
                    break;
                }
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    println!(
        "[Shikimori] готово. Сохранено: {}, пропущено без RU: {}",
        total_saved, total_skipped_no_ru
    );
    Ok(())
}

/// Обновляет запись в БД. Ищет по `title_romaji` или `title_english`.
/// Если не находит — создаёт новую с отрицательным id.
fn upsert_shikimori(
    conn: &Connection,
    item: &shikimori::ShikiAnime,
) -> Result<(), rusqlite::Error> {
    let name = item.name.as_deref().unwrap_or("");
    let russian = item.russian.as_deref().unwrap_or("");

    // Двойная защита — не должны попасть сюда без русского
    if russian.trim().is_empty() {
        return Ok(());
    }

    // Ищем запись в AniList
    let existing: Option<i64> = conn
        .query_row(
            "SELECT anilist_id FROM anime
             WHERE title_romaji = ?1 OR title_english = ?1
             LIMIT 1",
            [name],
            |r| r.get(0),
        )
        .ok();

    let studios_json = item
        .studios
        .as_ref()
        .map(|s| serde_json::to_string(s).unwrap_or_default());

    if let Some(id) = existing {
        // Обогащаем существующую запись AniList
        conn.execute(
            "UPDATE anime SET
                title_russian = ?1,
                description_ru = COALESCE(?2, description_ru),
                shikimori_id = ?3,
                shikimori_studios_json = ?4,
                shikimori_synced_at = strftime('%s','now')
             WHERE anilist_id = ?5",
            rusqlite::params![
                item.russian,
                item.description,
                item.id,
                studios_json,
                id,
            ],
        )?;
    } else {
        // Нет в AniList — создаём запись только для русского
        let fake_id = -item.id;
        conn.execute(
            "INSERT OR IGNORE INTO anime (
                anilist_id, shikimori_id,
                title_romaji, title_russian,
                format, status, description_ru,
                episodes,
                cover_large,
                shikimori_studios_json,
                shikimori_synced_at
            ) VALUES (
                ?1, ?2, ?3, ?4,
                ?5, ?6, ?7,
                ?8,
                ?9,
                ?10,
                strftime('%s','now')
            )",
            rusqlite::params![
                fake_id,
                item.id,
                item.name,
                item.russian,
                item.kind,
                item.status,
                item.description,
                item.episodes,
                item.image
                    .as_ref()
                    .and_then(|v| v.get("original"))
                    .and_then(|x| x.as_str()),
                studios_json,
            ],
        )?;
    }

    Ok(())
}