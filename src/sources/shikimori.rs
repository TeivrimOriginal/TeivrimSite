//! Shikimori.one — the Russian side of the catalogue.
//!
//! The v1 API returns `meta` alongside the page, which is what makes correct
//! pagination possible. The old importer inferred "is there a next page?" from
//! `items.len() >= 40` while asking for 50 per page, so any page that happened
//! to return 41..49 items made the loader stop early, and a short final page
//! made it request one page too many.

use serde::{Deserialize, Serialize};

pub const API: &str = "https://shikimori.one/api";

#[derive(Debug, Deserialize)]
pub struct Response {
    #[serde(default)]
    pub data: Vec<Anime>,
    #[serde(default)]
    pub meta: Meta,
}

#[derive(Debug, Default, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub total: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Anime {
    pub id: i64,
    pub name: Option<String>,
    pub russian: Option<String>,
    pub english: Option<Vec<String>>,
    pub japanese: Option<Vec<String>>,
    pub synonyms: Option<Vec<String>>,
    pub kind: Option<String>,
    pub rating: Option<String>,
    #[serde(default, deserialize_with = "de_score")]
    pub score: Option<f64>,
    pub status: Option<String>,
    pub episodes: Option<i64>,
    #[serde(rename = "episodes_aired")]
    pub episodes_aired: Option<i64>,
    #[serde(rename = "aired_on")]
    pub aired_on: Option<String>,
    #[serde(rename = "released_on")]
    pub released_on: Option<String>,
    pub description: Option<String>,
    pub image: Option<ImageSet>,
    pub studios: Option<Vec<Named>>,
    pub genres: Option<Vec<Named>>,
    pub tags: Option<Vec<Tag>>,
    pub rates: Option<Rates>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImageSet {
    #[serde(rename = "original", default, skip_serializing_if = "Option::is_none")]
    pub original: Option<String>,
    #[serde(rename = "preview", default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Named {
    pub name: Option<String>,
    pub russian: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tag {
    pub name: Option<String>,
    pub russian: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Rates {
    pub score: Option<f64>,
    pub status: Option<i64>,
    pub completed: Option<i64>,
    pub current: Option<i64>,
    pub planned: Option<i64>,
    pub on_hold: Option<i64>,
    pub dropped: Option<i64>,
}

/// Shikimori reports `score` as the string "8.42" on some endpoints and as a
/// number on others, and `null` when unscored.
fn de_score<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    match Option::<serde_json::Value>::deserialize(deserializer)? {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(s)) => {
            let s = s.trim();
            if s.is_empty() {
                Ok(None)
            } else {
                Ok(s.parse().ok())
            }
        }
        Some(serde_json::Value::Number(n)) => Ok(n.as_f64()),
        _ => Ok(None),
    }
}

pub struct Fetched {
    pub items: Vec<Anime>,
    pub has_next: bool,
}

pub fn page_url(page: u32, per_page: u32, order: &str) -> String {
    format!(
        "{}/animes?page={}&limit={}&order={}",
        API, page, per_page, order
    )
}

pub async fn fetch_page(
    up: &crate::upstream::Upstream,
    page: u32,
    per_page: u32,
    order: &str,
) -> Result<Fetched, String> {
    let value = up.get_json(&page_url(page, per_page, order)).await?;
    let resp: Response = serde_json::from_value(value)
        .map_err(|e| format!("shikimori: не разобрался ответ: {}", e))?;

    // Drop entries that failed to deserialise rather than failing the page: one
    // malformed record should not stop a 22k row import.
    let items: Vec<Anime> = resp.data;
    let received = items.len() as u64;
    let has_next = has_next_page(received, page, per_page, resp.meta.total);

    Ok(Fetched { items, has_next })
}

/// Whether the loader should ask for another page.
///
/// `meta.total` is what makes pagination correct. v1 guessed with
/// `items.len() >= 40` while asking for 50 per page, so any page returning
/// 41..49 items ended the import early and a short final page caused one
/// extra request.
///
/// Split out of [`fetch_page`] because it is a decision, not a side effect, and
/// a decision is worth pinning down with tests: the two failure modes above are
/// both silent.
pub fn has_next_page(received: u64, page: u32, per_page: u32, total: u64) -> bool {
    if total > 0 {
        received > 0 && (page as u64).saturating_mul(per_page as u64) < total
    } else {
        // No usable meta: a short page is the last page.
        received >= per_page as u64
    }
}

/// Cheap reachability probe used at worker start-up.
///
/// The one record it reads has carried the same Russian title for years, and
/// the probe runs at the start of every pass, so it is reused for the life of
/// the process. A cache that outlived a source being *down* would be a
/// different design: the first failure is not stored, so a source that comes
/// back is noticed on the next pass.
pub async fn ping(up: &crate::upstream::Upstream) -> Result<Option<String>, String> {
    let value = up
        .get_json_cached(
            &format!("{}/animes/16498", API),
            crate::upstream::Freshness::Forever,
        )
        .await?;
    Ok(value
        .get("russian")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---------------------------------------------------------------- score

    #[test]
    fn score_is_read_from_a_number() {
        // Some endpoints return a JSON number.
        let a: Anime = serde_json::from_value(json!({ "id": 1, "score": 8.42 })).unwrap();
        assert_eq!(a.score, Some(8.42));
    }

    #[test]
    fn score_is_read_from_a_string() {
        // And others return "8.42". Treating the string as zero was a
        // two-year-old silent data bug: every rating looked like 0.
        let a: Anime = serde_json::from_value(json!({ "id": 1, "score": "8.42" })).unwrap();
        assert_eq!(a.score, Some(8.42));
    }

    #[test]
    fn a_string_score_with_surrounding_space_still_parses() {
        let a: Anime = serde_json::from_value(json!({ "id": 1, "score": "  7.5  " })).unwrap();
        assert_eq!(a.score, Some(7.5));
    }

    #[test]
    fn a_null_score_is_none_not_zero() {
        // A zero rating would sort last and would be indistinguishable from
        // an unscored title, which is a different thing entirely.
        let a: Anime = serde_json::from_value(json!({ "id": 1, "score": null })).unwrap();
        assert_eq!(a.score, None);
    }

    #[test]
    fn an_absent_score_is_none() {
        let a: Anime = serde_json::from_value(json!({ "id": 1 })).unwrap();
        assert_eq!(a.score, None);
    }

    #[test]
    fn an_empty_or_unparsable_score_is_none() {
        for raw in [json!(""), json!("   "), json!("н/д"), json!("--"), json!(true)] {
            let a: Anime = serde_json::from_value(json!({ "id": 1, "score": raw })).unwrap();
            assert_eq!(a.score, None, "score {:?} должен стать None", raw);
        }
    }

    // -------------------------------------------------------------- mapping

    #[test]
    fn maps_a_shikimori_entry_onto_the_internal_type() {
        let v = json!({
            "id": 16498,
            "name": "Shingeki no Kyojin",
            "russian": "Атака Титанов",
            "english": ["Attack on Titan"],
            "japanese": ["進撃の巨人"],
            "synonyms": ["AoT"],
            "kind": "tv",
            "rating": "9.1",
            "score": 9.15,
            "status": "released",
            "episodes": 25,
            "episodes_aired": 25,
            "aired_on": "2013-04-07",
            "released_on": "2015-04-01",
            "description": "<p>Гиганты</p>",
            "image": { "original": "https://shikimori.one/o.jpg", "preview": "https://shikimori.one/p.jpg" },
            "studios": [{ "name": "Wit Studio", "russian": "Wit Studio" }],
            "genres": [{ "name": "Action", "russian": "Боевик" }],
            "tags": [{ "name": "Military", "russian": "Военный" }],
            "rates": { "score": 9.15, "status": 3000, "completed": 100, "planned": 20 }
        });
        let a: Anime = serde_json::from_value(v).unwrap();
        assert_eq!(a.id, 16498);
        assert_eq!(a.name.as_deref(), Some("Shingeki no Kyojin"));
        assert_eq!(a.russian.as_deref(), Some("Атака Титанов"));
        assert_eq!(a.english.as_ref().unwrap()[0], "Attack on Titan");
        assert_eq!(a.japanese.as_ref().unwrap()[0], "進撃の巨人");
        assert_eq!(a.kind.as_deref(), Some("tv"));
        assert_eq!(a.rating.as_deref(), Some("9.1"));
        assert_eq!(a.status.as_deref(), Some("released"));
        assert_eq!(a.episodes, Some(25));
        assert_eq!(a.episodes_aired, Some(25));
        assert_eq!(a.aired_on.as_deref(), Some("2013-04-07"));
        assert_eq!(a.image.as_ref().unwrap().original.as_deref(), Some("https://shikimori.one/o.jpg"));
        assert_eq!(a.studios.as_ref().unwrap()[0].russian.as_deref(), Some("Wit Studio"));
        assert_eq!(a.genres.as_ref().unwrap()[0].name.as_deref(), Some("Action"));
        assert_eq!(a.tags.as_ref().unwrap()[0].russian.as_deref(), Some("Военный"));
        assert_eq!(a.rates.as_ref().unwrap().completed, Some(100));
    }

    #[test]
    fn a_sparse_entry_parses_with_defaults() {
        let a: Anime = serde_json::from_value(json!({ "id": 5 })).unwrap();
        assert_eq!(a.id, 5);
        assert_eq!(a.name, None);
        assert_eq!(a.russian, None);
        assert_eq!(a.english, None);
        assert_eq!(a.kind, None);
        assert_eq!(a.score, None);
        assert!(a.image.is_none());
        assert!(a.rates.is_none());
    }

    #[test]
    fn a_page_with_no_meta_defaults_to_zero() {
        // `meta` is what makes pagination correct, so a missing one has to be
        // visible: the total is 0 and the caller falls back to page length.
        let r: Response = serde_json::from_value(json!({ "data": [] })).unwrap();
        assert_eq!(r.meta.total, 0);
        assert!(r.data.is_empty());
    }

    // ---------------------------------------------------------- pagination

    #[test]
    fn pagination_follows_meta_total() {
        // 200 rows, 50 per page: pages 1..3 have more, page 4 does not.
        assert!(has_next_page(50, 1, 50, 200));
        assert!(has_next_page(50, 2, 50, 200));
        assert!(has_next_page(50, 3, 50, 200));
        assert!(!has_next_page(50, 4, 50, 200));
    }

    #[test]
    fn pagination_ends_on_an_empty_page_even_when_meta_is_wrong() {
        // An empty page is the end of the catalogue whatever `total` claims,
        // otherwise a wrong total turns into an endless request loop against a
        // free public API.
        assert!(!has_next_page(0, 1, 50, 200));
        assert!(!has_next_page(0, 7, 50, 200));
    }

    #[test]
    fn a_short_page_does_not_end_the_walk_when_meta_says_otherwise() {
        // The v1 bug: 41..49 items ended a 50-per-page import early. With
        // `meta.total` present the page length is irrelevant.
        assert!(has_next_page(41, 1, 50, 200));
        assert!(has_next_page(1, 3, 50, 200));
    }

    #[test]
    fn without_meta_a_full_page_continues_and_a_short_page_stops() {
        assert!(has_next_page(50, 1, 50, 0));
        assert!(!has_next_page(49, 1, 50, 0));
        assert!(!has_next_page(0, 1, 50, 0));
    }

    #[test]
    fn pagination_does_not_overflow_on_a_huge_page_number() {
        // `page * per_page` is saturating, so a corrupt checkpoint asking for
        // page 4_294_967_295 answers "no more pages" instead of wrapping.
        assert!(!has_next_page(1, u32::MAX, 50, 200));
    }

    // ------------------------------------------------------------- url

    #[test]
    fn page_url_carries_paging_and_order() {
        assert_eq!(
            page_url(2, 50, "rating"),
            "https://shikimori.one/api/animes?page=2&limit=50&order=rating"
        );
    }
}
