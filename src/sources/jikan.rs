use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Deserialize)]
pub struct JikanResp {
    pub data: Vec<JikanAnime>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct JikanAnime {
    pub mal_id: i64,
    pub title: Option<String>,
    pub title_english: Option<String>,
    pub title_japanese: Option<String>,
    pub title_synonyms: Option<Vec<String>>,
    #[serde(rename = "type")]
    pub format: Option<String>,
    pub source: Option<String>,
    pub status: Option<String>,
    pub airing: Option<bool>,
    pub episodes: Option<i64>,
    pub duration: Option<String>,
    pub rating: Option<String>,
    pub score: Option<f64>,
    pub scored_by: Option<i64>,
    pub rank: Option<i64>,
    pub popularity: Option<i64>,
    pub members: Option<i64>,
    pub favorites: Option<i64>,
    pub synopsis: Option<String>,
    pub background: Option<String>,
    pub season: Option<String>,
    pub year: Option<i64>,
    pub images: Option<serde_json::Value>,
    pub trailer: Option<serde_json::Value>,
    pub genres: Option<Vec<Named>>,
    pub themes: Option<Vec<Named>>,
    pub demographics: Option<Vec<Named>>,
    pub studios: Option<Vec<Named>>,
    pub producers: Option<Vec<Named>>,
    pub licensors: Option<Vec<Named>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Named {
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TopResp {
    data: Vec<JikanAnime>,
    pagination: Option<JikanPagination>,
}

#[derive(Debug, Deserialize)]
struct JikanPagination {
    #[serde(rename = "has_next_page")]
    has_next_page: Option<bool>,
    #[serde(rename = "last_visible_page")]
    last_visible_page: Option<i64>,
}

pub async fn fetch_top_page(
    client: &reqwest::Client,
    page: u32,
) -> Result<(Vec<JikanAnime>, bool), Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("https://api.jikan.moe/v4/top/anime?page={}&limit=25", page);

    let resp = client.get(&url).send().await?;
    let status = resp.status();

    if status.as_u16() == 429 {
        eprintln!("[Jikan RATE LIMIT] 429. Ждём 3 сек...");
        tokio::time::sleep(Duration::from_secs(3)).await;
        return Err("429".into());
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {}: {}", status, body).into());
    }

    let parsed: TopResp = resp.json().await?;
    let has_next = parsed
        .pagination
        .as_ref()
        .and_then(|p| p.has_next_page)
        .unwrap_or(false);

    Ok((parsed.data, has_next))
}

pub async fn fetch_full_by_mal(
    client: &reqwest::Client,
    mal_id: i64,
) -> Result<Option<serde_json::Value>, Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("https://api.jikan.moe/v4/anime/{}/full", mal_id);
    let resp = client.get(&url).send().await?;

    if resp.status().as_u16() == 404 {
        return Ok(None);
    }
    if resp.status().as_u16() == 429 {
        tokio::time::sleep(Duration::from_secs(3)).await;
        return Err("429".into());
    }
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()).into());
    }

    let v: serde_json::Value = resp.json().await?;
    Ok(v.get("data").cloned())
}