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
/// Flattened into the shape `api::detail::parse_people` reads.
///
/// Visible outside this module only so that reader can be tested against a
/// real payload instead of against a fixture that agrees with it by
/// construction. That pairing is the whole point: the two sides drifted once
/// and the detail page silently lost its cast section.
pub fn store_cast(value: &serde_json::Value) -> Option<String> {
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

/// Flattened into the shape `api::detail::parse_people` reads. See
/// [`store_cast`].
pub fn store_staff(value: &serde_json::Value) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A JSON:API character page as Kitsu returns it: relationships in `data`,
    /// the people and characters they point at in `included`.
    fn characters_page() -> serde_json::Value {
        json!({
            "data": [
                { "id": "c1", "role": "Main", "voiceActors": [
                    { "id": "p1", "language": "English", "name": "VA EN" },
                    { "id": "p2", "language": "Japanese", "name": "VA JP" }
                ]},
                { "id": "c2", "role": "Supporting", "voiceActors": [] },
                { "id": "c-missing", "role": "X", "voiceActors": [] }
            ],
            "included": [
                { "type": "characters", "id": "c1",
                  "attributes": { "name": "Eren Yeager", "image": { "large": "eren.jpg" } } },
                { "type": "characters", "id": "c2",
                  "attributes": { "name": "Armin", "image": null } },
                { "type": "people", "id": "p1", "attributes": { "name": "VA EN" } },
                { "type": "people", "id": "p2", "attributes": { "name": "VA JP" } }
            ]
        })
    }

    fn parsed(raw: &str) -> Vec<serde_json::Value> {
        serde_json::from_str(raw).unwrap()
    }

    // ---------------------------------------------------------------- cast

    #[test]
    fn a_character_page_is_flattened_into_the_wire_shape() {
        let out = store_cast(&characters_page()).unwrap();
        let v = parsed(&out);
        assert_eq!(v.len(), 2, "запись без персонажа в included пропускается");
        assert_eq!(v[0]["name"], json!("Eren Yeager"));
        assert_eq!(v[0]["image"], json!("eren.jpg"));
        assert_eq!(v[0]["role"], json!("Main"));
        assert_eq!(v[1]["name"], json!("Armin"));
        assert_eq!(v[1]["image"], json!(null));
    }

    #[test]
    fn the_japanese_voice_actor_wins() {
        // Kitsu lists every dub; the Japanese one is the one the detail page
        // shows first.
        let v = parsed(&store_cast(&characters_page()).unwrap());
        assert_eq!(v[0]["voice_actor"], json!("VA JP"));
    }

    #[test]
    fn a_character_with_no_voice_actor_is_still_kept() {
        // Two thirds of the catalogue has no cast enrichment at all; dropping
        // the character because of a missing voice would empty the section.
        let v = parsed(&store_cast(&characters_page()).unwrap());
        assert_eq!(v[1]["voice_actor"], json!(null));
    }

    #[test]
    fn a_voice_actor_that_is_not_included_is_ignored() {
        // A dangling relationship must not produce a character with no name
        // instead of a named one with no voice.
        let page = json!({
            "data": [{ "id": "c1", "role": "Main",
                       "voiceActors": [{ "id": "p-x", "language": "Japanese", "name": "X" }] }],
            "included": [{ "type": "characters", "id": "c1", "attributes": { "name": "Eren" } }]
        });
        let v = parsed(&store_cast(&page).unwrap());
        assert_eq!(v[0]["name"], json!("Eren"));
        assert_eq!(v[0]["voice_actor"], json!(null));
    }

    #[test]
    fn a_character_without_a_name_is_skipped() {
        let page = json!({
            "data": [{ "id": "c1", "role": "Main" }],
            "included": [{ "type": "characters", "id": "c1", "attributes": {} }]
        });
        assert_eq!(parsed(&store_cast(&page).unwrap()).len(), 0);
    }

    #[test]
    fn an_empty_cast_page_is_an_empty_array_not_nothing() {
        // `[]` and NULL are different to the loader: NULL means "never fetched",
        // `[]` means "fetched, nobody in the cast".
        assert_eq!(store_cast(&json!({ "data": [] })).unwrap(), "[]");
    }

    #[test]
    fn a_cast_page_of_the_wrong_shape_is_rejected_rather_than_half_read() {
        assert!(store_cast(&json!({})).is_none());
        assert!(store_cast(&json!({ "data": "not an array" })).is_none());
        assert!(store_cast(&json!([1, 2, 3])).is_none());
    }

    // --------------------------------------------------------------- staff

    #[test]
    fn a_staff_page_carries_positions_as_a_list() {
        let page = json!({
            "data": [{ "id": "s1" }],
            "included": [
                { "type": "people", "id": "s1", "attributes": {
                    "name": "Sasha Ishida",
                    "image": { "large": "sasha.jpg" },
                    "role": { "attributes": { "title": "Director" } }
                }}
            ]
        });
        let v = parsed(&store_staff(&page).unwrap());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0]["name"], json!("Sasha Ishida"));
        assert_eq!(v[0]["image"], json!("sasha.jpg"));
        assert_eq!(v[0]["positions"][0], json!("Director"));
    }

    #[test]
    fn a_person_without_a_role_gets_an_empty_positions_list() {
        // The detail page iterates positions unconditionally, so the array has
        // to exist.
        let page = json!({
            "data": [{ "id": "s1" }],
            "included": [{ "type": "people", "id": "s1", "attributes": { "name": "Nobody" } }]
        });
        let v = parsed(&store_staff(&page).unwrap());
        assert_eq!(v[0]["positions"], json!([]));
    }

    #[test]
    fn a_staff_page_of_the_wrong_shape_is_rejected() {
        assert!(store_staff(&json!({ "included": [] })).is_none());
        assert!(store_staff(&json!({ "data": {} })).is_none());
    }

    #[test]
    fn a_person_without_a_name_is_skipped() {
        let page = json!({
            "data": [{ "id": "s1" }],
            "included": [{ "type": "people", "id": "s1", "attributes": { "image": null } }]
        });
        assert_eq!(parsed(&store_staff(&page).unwrap()).len(), 0);
    }

    #[test]
    fn the_two_shapes_do_not_leak_into_each_other() {
        // `characters` and `people` live in the same `included` array, and both
        // helpers search it by type: without that check every character would
        // get a person's name.
        let page = json!({
            "data": [{ "id": "x1" }],
            "included": [
                { "type": "characters", "id": "x1", "attributes": { "name": "Eren" } },
                { "type": "people", "id": "x1", "attributes": { "name": "Sasha" } }
            ]
        });
        let cast = parsed(&store_cast(&page).unwrap());
        let staff = parsed(&store_staff(&page).unwrap());
        assert_eq!(cast[0]["name"], json!("Eren"));
        assert_eq!(staff[0]["name"], json!("Sasha"));
    }
}

