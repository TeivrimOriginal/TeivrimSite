use rusqlite::Connection;
use std::time::Duration;

/// Перевод одного текста через неофициальный Google Translate.
/// Бесплатно, без ключа. Лимит ~5 запросов/сек — иначе бан IP.
pub async fn translate(
    client: &reqwest::Client,
    text: &str,
    from: &str,
    to: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if text.trim().is_empty() {
        return Ok(String::new());
    }

    let encoded: String = urlencoding::encode(text).to_string();
    let url = format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl={}&tl={}&dt=t&q={}",
        from, to, encoded
    );

    let resp = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await?;

    if resp.status().as_u16() == 429 {
        return Err("429 — Google забаннил, ждём".into());
    }
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()).into());
    }

    let v: serde_json::Value = resp.json().await?;

    // Структура: [ [ ["переведённый кусок", "оригинал", ...], ... ], ... ]
    let mut out = String::new();
    if let Some(arr) = v.get(0).and_then(|x| x.as_array()) {
        for piece in arr {
            if let Some(s) = piece.get(0).and_then(|x| x.as_str()) {
                out.push_str(s);
            }
        }
    }

    if out.is_empty() {
        return Err("Пустой перевод".into());
    }

    Ok(out)
}

/// Один проход по БД: переводит всё, у чего translated_at IS NULL.
/// Возвращает, сколько записей переведено ПОЛНОСТЬЮ за этот проход.
///
/// Флаг finished в sync_state больше не используется. Раньше переводчик стартовал через 60 сек,
/// когда AniList успел залить всего пару сотен записей, видел "всё переведено", ставил finished=1
/// и больше никогда не запускался, хотя загрузчики продолжали добавлять записи.
pub async fn translate_pending() -> Result<i64, Box<dyn std::error::Error>> {
    println!("\n========== Переводчик (Google) ==========");

    let client = reqwest::Client::builder()
        .user_agent("AnimeLoader/2.0")
        .timeout(Duration::from_secs(30))
        .build()?;

    let conn = crate::db::open()?;
    crate::db::ensure_schema(&conn)?;

    let mut total_all: i64 = conn
        .query_row(
            "SELECT COALESCE(total_saved,0) FROM sync_state WHERE source='translate' AND task='machine'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let mut last_id: i64 = 0;
    let mut done_now: i64 = 0;
    let mut fails_in_row: u32 = 0;

    loop {
        let batch: Vec<(i64, Option<String>, Option<String>, Option<String>)> = {
            let mut stmt = conn.prepare(
                "SELECT anilist_id, title_romaji, title_english, description
                 FROM anime
                 WHERE anilist_id > ?1
                   AND translated_at IS NULL
                   AND (title_romaji IS NOT NULL OR title_english IS NOT NULL)
                 ORDER BY anilist_id ASC
                 LIMIT 200",
            )?;

            let rows = stmt
                .query_map([last_id], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                    ))
                })?
                .filter_map(|x| x.ok())
                .collect();

            rows
        };

        if batch.is_empty() {
            println!("[Translate] в этом проходе больше нечего переводить. Переведено сейчас: {}", done_now);
            crate::db::save_checkpoint(&conn, "translate", "machine", last_id, total_all, false)?;
            break;
        }

        for (anilist_id, romaji, english, description) in &batch {
            // Английское название переводится нормально ("Attack on Titan" -> "Атака титанов"),
            // а romaji ("Shingeki no Kyojin") Google просто транслитерирует в кашу.
            let title_src = english
                .as_deref()
                .filter(|s| !s.is_empty())
                .or_else(|| romaji.as_deref().filter(|s| !s.is_empty()));

            let mut title_ru: Option<String> = None;
            if let Some(t) = title_src {
                match translate(&client, t, "en", "ru").await {
                    Ok(v) => {
                        title_ru = Some(v);
                        fails_in_row = 0;
                    }
                    Err(e) => {
                        fails_in_row += 1;
                        eprintln!("[Translate] title error (id={}): {}", anilist_id, e);
                        if fails_in_row >= 20 {
                            eprintln!("[Translate] 20 ошибок подряд (Google режет запросы?) — прерываю проход, повторю позже");
                            crate::db::save_checkpoint(&conn, "translate", "machine", last_id, total_all, false)?;
                            return Ok(done_now);
                        }
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }

            // Описание. desc_ok = false, если перевод описания упал — тогда запись
            // НЕ помечается переведённой и будет повторена в следующем проходе.
            let mut desc_ru: Option<String> = None;
            let mut desc_ok = true;
            if let Some(d) = description {
                let d_trim = d.trim();
                if !d_trim.is_empty() {
                    let d_cut: String = d_trim.chars().take(4500).collect();
                    match translate(&client, &d_cut, "en", "ru").await {
                        Ok(v) => {
                            desc_ru = Some(v);
                        }
                        Err(e) => {
                            desc_ok = false;
                            eprintln!("[Translate] desc error (id={}): {}", anilist_id, e);
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            }

            conn.execute(
                "UPDATE anime SET
                    title_ru_machine = COALESCE(?1, title_ru_machine),
                    description_ru_machine = COALESCE(?2, description_ru_machine),
                    translated_at = CASE WHEN ?3 = 1 THEN strftime('%s','now') ELSE translated_at END
                 WHERE anilist_id = ?4",
                rusqlite::params![title_ru, desc_ru, desc_ok as i64, anilist_id],
            )?;

            if desc_ok {
                total_all += 1;
                done_now += 1;
                if done_now % 50 == 0 {
                    println!("[Translate] переведено: {}", done_now);
                }
            }
        }

        // Курсор двигаем вперёд, иначе записи с ошибкой попадают в каждую следующую пачку
        last_id = batch.last().map(|x| x.0).unwrap_or(last_id);
        crate::db::save_checkpoint(&conn, "translate", "machine", last_id, total_all, false)?;

        // Пауза между пачками — 5 сек
        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    Ok(done_now)
}
