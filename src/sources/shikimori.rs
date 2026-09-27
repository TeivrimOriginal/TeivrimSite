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

    // `meta.total` is what makes pagination correct. v1 guessed with
    // `items.len() >= 40` while asking for 50 per page, so any page returning
    // 41..49 items ended the import early and a short final page caused one
    // extra request.
    let has_next = if resp.meta.total > 0 {
        received > 0 && (page as u64).saturating_mul(per_page as u64) < resp.meta.total
    } else {
        // No usable meta: a short page is the last page.
        received >= per_page as u64
    };

    Ok(Fetched { items, has_next })
}

/// Cheap reachability probe used at worker start-up.
pub async fn ping(up: &crate::upstream::Upstream) -> Result<Option<String>, String> {
    let value = up.get_json(&format!("{}/animes/16498", API)).await?;
    Ok(value
        .get("russian")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string()))
}
