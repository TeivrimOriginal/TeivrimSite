use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Deserialize)]
pub struct AniListResp {
    pub data: AniListData,
}

#[derive(Debug, Deserialize)]
pub struct AniListData {
    #[serde(rename = "Page")]
    pub page: AniListPage,
}

#[derive(Debug, Deserialize)]
pub struct AniListPage {
    #[serde(rename = "pageInfo")]
    pub page_info: PageInfo,
    pub media: Vec<Media>,
}

#[derive(Debug, Deserialize)]
pub struct PageInfo {
    #[serde(rename = "hasNextPage")]
    pub has_next_page: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    pub id: i64,
    pub id_mal: Option<i64>,
    pub title: Title,
    pub synonyms: Option<Vec<String>>,
    pub format: Option<String>,
    pub status: Option<String>,
    pub description: Option<String>,
    pub duration: Option<i64>,
    pub episodes: Option<i64>,
    pub chapters: Option<i64>,
    pub volumes: Option<i64>,
    pub country_of_origin: Option<String>,
    pub is_adult: Option<bool>,
    pub is_licensed: Option<bool>,
    pub start_date: Option<FuzzyDate>,
    pub end_date: Option<FuzzyDate>,
    pub season: Option<String>,
    pub season_year: Option<i64>,
    pub average_score: Option<i64>,
    pub mean_score: Option<i64>,
    pub popularity: Option<i64>,
    pub favourites: Option<i64>,
    pub trending: Option<i64>,
    pub cover_image: Option<CoverImage>,
    pub banner_image: Option<String>,
    pub trailer: Option<Trailer>,
    pub genres: Option<Vec<String>>,
    pub tags: Option<Vec<Tag>>,
    pub studios: Option<Studios>,
    pub relations: Option<Relations>,
    pub external_links: Option<Vec<ExternalLink>>,
    pub streaming_episodes: Option<Vec<StreamingEpisode>>,
    pub recommendations: Option<Recommendations>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Title {
    pub romaji: Option<String>,
    pub english: Option<String>,
    pub native: Option<String>,
    #[serde(rename = "userPreferred")]
    pub user_preferred: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FuzzyDate {
    pub year: Option<i64>,
    pub month: Option<i64>,
    pub day: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CoverImage {
    pub extra_large: Option<String>,
    pub large: Option<String>,
    pub medium: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Trailer {
    pub id: Option<String>,
    pub site: Option<String>,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub name: Option<String>,
    pub rank: Option<i64>,
    pub is_media_spoiler: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Studios {
    pub nodes: Option<Vec<Studio>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Studio {
    pub name: Option<String>,
    pub is_animation_studio: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Relations {
    pub edges: Option<Vec<RelationEdge>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RelationEdge {
    pub relation_type: Option<String>,
    pub node: Option<RelationNode>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RelationNode {
    pub id: i64,
    pub title: Option<Title>,
    pub format: Option<String>,
    pub status: Option<String>,
    pub cover_image: Option<CoverImage>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ExternalLink {
    pub id: i64,
    pub url: Option<String>,
    pub site: Option<String>,
    #[serde(rename = "type")]
    pub link_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StreamingEpisode {
    pub title: Option<String>,
    pub thumbnail: Option<String>,
    pub url: Option<String>,
    pub site: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Recommendations {
    pub nodes: Option<Vec<RecommendationNode>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RecommendationNode {
    pub rating: Option<i64>,
    pub media_recommendation: Option<RecommendedMedia>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RecommendedMedia {
    pub id: i64,
    pub title: Option<Title>,
    pub format: Option<String>,
    pub cover_image: Option<CoverImage>,
}

pub fn build_query(page: u32, per_page: u32, sort: &str) -> String {
    format!(
        r#"{{
            Page(page: {}, perPage: {}) {{
                pageInfo {{ hasNextPage }}
                media(type: ANIME, sort: {}) {{
                    id idMal
                    title {{ romaji english native userPreferred }}
                    synonyms
                    format status description
                    duration episodes chapters volumes
                    countryOfOrigin isAdult isLicensed
                    startDate {{ year month day }}
                    endDate   {{ year month day }}
                    season seasonYear
                    averageScore meanScore popularity favourites trending
                    coverImage {{ extraLarge large medium color }}
                    bannerImage
                    trailer {{ id site thumbnail }}
                    genres
                    tags {{ name rank isMediaSpoiler }}
                    studios(isMain: true) {{ nodes {{ name isAnimationStudio }} }}
                    relations {{
                        edges {{
                            relationType
                            node {{
                                id
                                title {{ romaji english native }}
                                format status
                                coverImage {{ large }}
                            }}
                        }}
                    }}
                    externalLinks {{ id url site type }}
                    streamingEpisodes {{ title thumbnail url site }}
                    recommendations(sort: RATING_DESC, perPage: 10) {{
                        nodes {{
                            rating
                            mediaRecommendation {{
                                id
                                title {{ romaji english native }}
                                format
                                coverImage {{ large }}
                            }}
                        }}
                    }}
                }}
            }}
        }}"#,
        page, per_page, sort
    )
}

pub async fn fetch_page(
    client: &reqwest::Client,
    page: u32,
    per_page: u32,
    sort: &str,
) -> Result<(Vec<Media>, bool), Box<dyn std::error::Error + Send + Sync>> {
    let query = build_query(page, per_page, sort);

    let resp = client
        .post("https://graphql.anilist.co")
        .json(&serde_json::json!({ "query": query }))
        .send()
        .await?;

    let status = resp.status();

    if status.as_u16() == 429 {
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(60);
        eprintln!("[AniList RATE LIMIT] 429. Ждём {} сек...", retry_after);
        tokio::time::sleep(Duration::from_secs(retry_after)).await;
        return Err(format!("429 rate limited, retry after {}s", retry_after).into());
    }

    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("HTTP {}: {}", status, body).into());
    }

    let parsed: AniListResp = resp.json().await?;
    let has_next = parsed.data.page.page_info.has_next_page;
    Ok((parsed.data.page.media, has_next))
}

pub fn media_to_json_pieces(m: &Media) -> Value {
    let studios_json = m
        .studios
        .as_ref()
        .and_then(|s| s.nodes.as_ref())
        .map(|nodes| {
            serde_json::json!(nodes
                .iter()
                .map(|n| serde_json::json!({
                    "name": n.name,
                    "isAnimationStudio": n.is_animation_studio
                }))
                .collect::<Vec<_>>())
        })
        .unwrap_or(Value::Null);

    let relations_json = m
        .relations
        .as_ref()
        .and_then(|r| r.edges.as_ref())
        .map(|edges| {
            serde_json::json!(edges
                .iter()
                .filter_map(|e| e.node.as_ref().map(|n| serde_json::json!({
                    "relationType": e.relation_type,
                    "id": n.id,
                    "title": n.title,
                    "format": n.format,
                    "status": n.status,
                    "coverImage": n.cover_image.as_ref().and_then(|c| c.large.clone()),
                })))
                .collect::<Vec<_>>())
        })
        .unwrap_or(Value::Null);

    let external_links_json = m
        .external_links
        .as_ref()
        .map(|links| {
            serde_json::json!(links
                .iter()
                .map(|l| serde_json::json!({
                    "id": l.id, "url": l.url, "site": l.site, "type": l.link_type
                }))
                .collect::<Vec<_>>())
        })
        .unwrap_or(Value::Null);

    let streaming_json = m
        .streaming_episodes
        .as_ref()
        .map(|eps| {
            serde_json::json!(eps
                .iter()
                .map(|e| serde_json::json!({
                    "title": e.title, "thumbnail": e.thumbnail, "url": e.url, "site": e.site
                }))
                .collect::<Vec<_>>())
        })
        .unwrap_or(Value::Null);

    let recs_json = m
        .recommendations
        .as_ref()
        .and_then(|r| r.nodes.as_ref())
        .map(|nodes| {
            serde_json::json!(nodes
                .iter()
                .filter_map(|n| n.media_recommendation.as_ref().map(|mm| serde_json::json!({
                    "rating": n.rating,
                    "id": mm.id,
                    "title": mm.title,
                    "format": mm.format,
                    "cover": mm.cover_image.as_ref().and_then(|c| c.large.clone()),
                })))
                .collect::<Vec<_>>())
        })
        .unwrap_or(Value::Null);

    serde_json::json!({
        "studios": studios_json,
        "relations": relations_json,
        "external_links": external_links_json,
        "streaming_episodes": streaming_json,
        "recommendations": recs_json,
    })
}