//! AniList GraphQL API — the primary source: ids, titles, scores, relations,
//! recommendations, tags and streaming links.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const API: &str = "https://graphql.anilist.co";

/// Sort enum values accepted by AniList. Pages are walked in several orders
/// because no single order reaches every row of a catalogue this size.
pub const SORTS: &[&str] = &[
    "POPULARITY_DESC",
    "SCORE_DESC",
    "START_DATE_DESC",
    "TRENDING_DESC",
    "FAVOURITES_DESC",
    "ID_DESC",
    "ID",
    "TITLE_ROMAJI",
];

#[derive(Debug, Deserialize)]
pub struct Response {
    pub data: Option<Data>,
    #[serde(default)]
    pub errors: Vec<GraphQLError>,
}

#[derive(Debug, Deserialize)]
pub struct Data {
    #[serde(rename = "Page")]
    pub page: Page,
}

#[derive(Debug, Deserialize)]
pub struct Page {
    #[serde(rename = "pageInfo")]
    pub page_info: PageInfo,
    #[serde(rename = "media")]
    pub media: Vec<Media>,
}

#[derive(Debug, Deserialize)]
pub struct PageInfo {
    #[serde(rename = "hasNextPage")]
    pub has_next_page: bool,
}

#[derive(Debug, Deserialize)]
pub struct GraphQLError {
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Media {
    pub id: i64,
    #[serde(rename = "idMal")]
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
    #[serde(rename = "countryOfOrigin")]
    pub country_of_origin: Option<String>,
    #[serde(rename = "isAdult")]
    pub is_adult: Option<bool>,
    #[serde(rename = "isLicensed")]
    pub is_licensed: Option<bool>,
    #[serde(rename = "startDate")]
    pub start_date: Option<FuzzyDate>,
    #[serde(rename = "endDate")]
    pub end_date: Option<FuzzyDate>,
    pub season: Option<String>,
    #[serde(rename = "seasonYear")]
    pub season_year: Option<i64>,
    #[serde(rename = "averageScore")]
    pub average_score: Option<i64>,
    #[serde(rename = "meanScore")]
    pub mean_score: Option<i64>,
    pub popularity: Option<i64>,
    pub favourites: Option<i64>,
    pub trending: Option<i64>,
    #[serde(rename = "coverImage")]
    pub cover_image: Option<CoverImage>,
    #[serde(rename = "bannerImage")]
    pub banner_image: Option<String>,
    pub trailer: Option<Trailer>,
    pub genres: Option<Vec<String>>,
    pub tags: Option<Vec<Tag>>,
    pub studios: Option<Connection<Studio>>,
    pub relations: Option<RelationConnection>,
    #[serde(rename = "externalLinks")]
    pub external_links: Option<Vec<ExternalLink>>,
    #[serde(rename = "streamingEpisodes")]
    pub streaming_episodes: Option<Vec<StreamingEpisode>>,
    pub recommendations: Option<RecommendationConnection>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Title {
    pub romaji: Option<String>,
    pub english: Option<String>,
    pub native: Option<String>,
    #[serde(rename = "userPreferred")]
    pub user_preferred: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FuzzyDate {
    pub year: Option<i64>,
    pub month: Option<i64>,
    pub day: Option<i64>,
}

impl FuzzyDate {
    /// ISO date, or the longest prefix that is actually known
    /// (`2024`, `2024-07`, `2024-07-14`).
    pub fn to_iso(&self) -> Option<String> {
        let y = self.year?;
        match (self.month, self.day) {
            (Some(m), Some(d)) => Some(format!("{:04}-{:02}-{:02}", y, m, d)),
            (Some(m), None) => Some(format!("{:04}-{:02}", y, m)),
            _ => Some(format!("{:04}", y)),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoverImage {
    #[serde(rename = "extraLarge")]
    pub extra_large: Option<String>,
    pub large: Option<String>,
    pub medium: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Trailer {
    pub id: Option<String>,
    pub site: Option<String>,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tag {
    pub name: Option<String>,
    pub rank: Option<i64>,
    #[serde(rename = "isMediaSpoiler")]
    pub is_media_spoiler: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Connection<T> {
    pub nodes: Option<Vec<T>>,
}

/// `relations` is the one connection that is edge-based rather than node-based.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelationConnection {
    pub edges: Option<Vec<RelationEdge>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Studio {
    pub name: Option<String>,
    #[serde(rename = "isAnimationStudio")]
    pub is_animation_studio: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelationEdge {
    #[serde(rename = "relationType")]
    pub relation_type: Option<String>,
    pub node: Option<RelationNode>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelationNode {
    pub id: i64,
    pub title: Option<Title>,
    pub format: Option<String>,
    pub status: Option<String>,
    #[serde(rename = "coverImage")]
    pub cover_image: Option<CoverImage>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExternalLink {
    pub id: i64,
    pub url: Option<String>,
    pub site: Option<String>,
    #[serde(rename = "type")]
    pub link_type: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StreamingEpisode {
    pub title: Option<String>,
    pub thumbnail: Option<String>,
    pub url: Option<String>,
    pub site: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecommendationConnection {
    pub nodes: Option<Vec<RecommendationNode>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecommendationNode {
    pub rating: Option<i64>,
    #[serde(rename = "mediaRecommendation")]
    pub media_recommendation: Option<RecommendedMedia>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecommendedMedia {
    pub id: i64,
    pub title: Option<Title>,
    pub format: Option<String>,
    #[serde(rename = "coverImage")]
    pub cover_image: Option<CoverImage>,
}

const MEDIA_FIELDS: &str = r#"
    id idMal
    title { romaji english native userPreferred }
    synonyms format status description(asHtml: false)
    duration episodes chapters volumes
    countryOfOrigin isAdult isLicensed
    startDate { year month day }
    endDate   { year month day }
    season seasonYear
    averageScore meanScore popularity favourites trending
    coverImage { extraLarge large medium color }
    bannerImage
    trailer { id site thumbnail }
    genres
    tags { name rank isMediaSpoiler }
    studios(isMain: true) { nodes { name isAnimationStudio } }
    relations {
        edges {
            relationType
            node { id title { romaji english native } format status coverImage { large } }
        }
    }
    externalLinks { id url site type }
    streamingEpisodes { title thumbnail url site }
    recommendations(sort: RATING_DESC, perPage: 12) {
        nodes { rating mediaRecommendation { id title { romaji english native } format coverImage { large } } }
    }
"#;

pub fn build_query(page: u32, per_page: u32, sort: &str) -> String {
    let sort = if SORTS.contains(&sort) { sort } else { "POPULARITY_DESC" };
    format!(
        r#"query {{
            Page(page: {page}, perPage: {per_page}) {{
                pageInfo {{ hasNextPage total }}
                media(type: ANIME, sort: [{sort}]) {{ {MEDIA_FIELDS} }}
            }}
        }}"#,
        page = page,
        per_page = per_page,
        sort = sort,
    )
}


pub struct Fetched {
    pub media: Vec<Media>,
    pub has_next: bool,
}

pub async fn fetch_page(
    up: &crate::upstream::Upstream,
    page: u32,
    per_page: u32,
    sort: &str,
) -> Result<Fetched, String> {
    let body = serde_json::json!({ "query": build_query(page, per_page, sort) });
    let value = up.post_json(API, &body).await?;
    parse_page(value)
}


fn parse_page(value: Value) -> Result<Fetched, String> {
    let resp: Response = serde_json::from_value(value).map_err(|e| format!("anilist: {}", e))?;
    if let Some(err) = resp.errors.first() {
        return Err(format!("anilist graphql: {}", err.message));
    }
    let data = resp.data.ok_or_else(|| "anilist: пустой ответ".to_string())?;
    Ok(Fetched {
        has_next: data.page.page_info.has_next_page,
        media: data.page.media,
    })
}

