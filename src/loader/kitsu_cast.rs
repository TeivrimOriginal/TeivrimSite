//! Cast and staff enrichment from Kitsu.
//!
//! The v1 code had columns for `jikan_characters_json` and `jikan_staff_json`
//! and a detail page that rendered both, but the function that filled them
//! (`enrich_jikan_full`) was never called from anywhere, so those sections were
//! permanently empty. This stage does the same job against Kitsu.
//!
//! It is bounded on purpose. Kitsu publishes no quota but throttles bursts, and
//! characters+staff is two requests per title, so a full 22k sweep would take
//! hours and hammer a free community API for data most visitors never scroll
//! to. The pass therefore covers the most popular titles first and stops.

use super::Ctx;
use crate::db;
use crate::error::{log_info, log_warn};
use crate::sources::kitsu;
use rusqlite::params;
use std::time::Instant;

const SOURCE: &str = "kitsu";
const TASK: &str = "enrich";

pub async fn run(ctx: Ctx, limit: i64) -> Result<(), String> {
    if limit <= 0 {
        log_info("[kitsu/cast] обогащение отключено (KITSU_ENRICH_LIMIT=0)");
        return Ok(());
    }

    let t0 = Instant::now();
    let cp = {
        let c = ctx.db.conn().map_err(|e| e.to_string())?;
        db::get_checkpoint(&c, SOURCE, TASK)
    };

    if cp.finished {
        log_info("[kitsu/cast] уже обогащено, пропускаю");
        return Ok(());
    }

    // Most popular first: those are the titles anyone actually opens, and the
    // ones where a missing cast list is most visible.
    let candidates: Vec<(String, String)> = {
        let c = ctx.db.conn().map_err(|e| e.to_string())?;
        let mut stmt = c
            .prepare(
                "SELECT uid, CAST(kitsu_id AS TEXT) FROM anime
                 WHERE kitsu_id IS NOT NULL
                   AND characters_json IS NULL
                   AND popularity IS NOT NULL
                 ORDER BY popularity ASC, kitsu_id ASC
                 LIMIT ?1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([limit], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?;
        rows.flatten().collect()
    };

    if candidates.is_empty() {
        let c = ctx.db.conn().map_err(|e| e.to_string())?;
        db::save_checkpoint(&c, SOURCE, TASK, 1, cp.total_saved, true).map_err(|e| e.to_string())?;
        return Ok(());
    }

    log_info(&format!(
        "[kitsu/cast] обогащаю {} записей (персонажи + создатели)",
        candidates.len()
    ));

    let total = candidates.len();
    let mut done = 0i64;
    let mut with_cast = 0i64;
    let mut errors = 0u32;
    let mut last_report = Instant::now();

    for (uid, kitsu_id) in candidates {
        if ctx.abort_requested() {
            log_info("[kitsu/cast] прервано по запросу");
            return Ok(());
        }

        let characters = kitsu::fetch_characters(&ctx.sources.kitsu, &kitsu_id).await;
        let staff = kitsu::fetch_staff(&ctx.sources.kitsu, &kitsu_id).await;

        if let (Err(e), Err(e2)) = (&characters, &staff) {
            errors += 1;
            log_warn(&format!("[kitsu/cast] {} (uid={}): {} / {}", kitsu_id, uid, e, e2));
            if errors >= 20 {
                log_warn("[kitsu/cast] 20 ошибок подряд, останавливаю проход");
                return Ok(());
            }
            continue;
        }
        errors = 0;

        let cast_json = characters
            .as_ref()
            .ok()
            .and_then(store_cast)
            .unwrap_or_else(|| "[]".to_string());
        let staff_json = staff
            .as_ref()
            .ok()
            .and_then(store_staff)
            .unwrap_or_else(|| "[]".to_string());

        let cast_count = serde_json::from_str::<Vec<serde_json::Value>>(&cast_json)
            .map(|v| v.len())
            .unwrap_or(0);

        let conn = ctx.db.conn().map_err(|e| e.to_string())?;
        let res = conn.execute(
            "UPDATE anime SET characters_json = ?1, staff_json = ?2, updated_at = ?3 WHERE uid = ?4",
            params![cast_json, staff_json, crate::error::now_ts(), uid],
        );
        drop(conn);

        match res {
            Ok(_) => {
                done += 1;
                if cast_count > 0 {
                    with_cast += 1;
                }
            }
            Err(e) => log_warn(&format!("[kitsu/cast] {}: {}", uid, e)),
        }

        if last_report.elapsed().as_secs() >= 20 {
            last_report = Instant::now();
            let pct = (done as f64 / total as f64 * 100.0).round() as i64;
            log_info(&format!(
                "[kitsu/cast] {}/{} ({}%), с персонажами: {}",
                done,
                total,
                pct,
                with_cast
            ));
        }
    }

    let conn = ctx.db.conn().map_err(|e| e.to_string())?;
    db::save_checkpoint(&conn, SOURCE, TASK, 1, cp.total_saved + done, true)
        .map_err(|e| e.to_string())?;

    log_info(&format!(
        "[kitsu/cast] готово за {}: {} записей, из них {} с персонажами",
        super::human_secs(t0.elapsed().as_secs()),
        done,
        with_cast
    ));
    Ok(())
}

/// Flattens a JSON:API character list into the flat shape the API serves.
fn store_cast(value: &serde_json::Value) -> Option<String> {
    let data = value.get("data")?.as_array()?;
    let included = value.get("included").and_then(|v| v.as_array());

    let find_included = |kind: &str, id: &str| -> Option<&serde_json::Value> {
        included?.iter().find(|r| {
            r.get("type").and_then(|t| t.as_str()) == Some(kind)
                && r.get("id").and_then(|t| t.as_str()) == Some(id)
        })
    };

    let mut out: Vec<serde_json::Value> = Vec::new();
    for rel in data {
        let character_id = rel.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let Some(char_res) = find_included("characters", character_id) else {
            continue;
        };
        let attrs = char_res.get("attributes");
        let name = attrs
            .and_then(|a| a.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let image = attrs
            .and_then(|a| a.get("image"))
            .and_then(|i| i.get("large"))
            .and_then(|l| l.as_str());

        // Voice actors come back as a relationship; resolving them through
        // `included` avoids N+1 requests.
        let mut voice = None;
        if let Some(va_list) = rel.get("voiceActors").and_then(|v| v.as_array()) {
            let japanese = va_list
                .iter()
                .find(|v| v.get("language").and_then(|l| l.as_str()) == Some("Japanese"))
                .or_else(|| va_list.first());
            if let Some(va) = japanese {
                let va_id = va.get("id").and_then(|v| v.as_str()).unwrap_or("");
                voice = find_included("people", va_id)
                    .and_then(|r| r.get("attributes"))
                    .and_then(|a| a.get("name"))
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string());
            }
        }

        out.push(serde_json::json!({
            "name": name,
            "image": image,
            "role": rel.get("role").and_then(|r| r.as_str()),
            "voice_actor": voice,
        }));
    }
    serde_json::to_string(&out).ok()
}

fn store_staff(value: &serde_json::Value) -> Option<String> {
    let data = value.get("data")?.as_array()?;
    let included = value.get("included").and_then(|v| v.as_array());

    let mut out: Vec<serde_json::Value> = Vec::new();
    for rel in data {
        let person_id = rel.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let person = included
            .and_then(|inc| {
                inc.iter().find(|r| {
                    r.get("type").and_then(|t| t.as_str()) == Some("people")
                        && r.get("id").and_then(|t| t.as_str()) == Some(person_id)
                })
            })
            .and_then(|r| r.get("attributes"));
        let name = person
            .and_then(|a| a.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let image = person
            .and_then(|a| a.get("image"))
            .and_then(|i| i.get("large"))
            .and_then(|l| l.as_str());
        let positions: Vec<String> = person
            .and_then(|a| a.get("role"))
            .and_then(|r| r.get("attributes"))
            .and_then(|r| r.get("title"))
            .and_then(|t| t.as_str())
            .map(|t| vec![t.to_string()])
            .unwrap_or_default();

        out.push(serde_json::json!({
            "name": name,
            "image": image,
            "positions": positions,
        }));
    }
    serde_json::to_string(&out).ok()
}

