//! Kitsu.io (JSON:API) — replaces the old Jikan/MyAnimeList importer.
//!
//! Why this source: Jikan is an unofficial MAL proxy and was unreachable from
//! the deployment network, while Kitsu is a first-party API and returns
//! everything MAL did in the old schema, plus more:
//!
//! * `averageRating` is already a 0..100 string, so it lines up with AniList's
//!   scale and no conversion is needed.
//! * `mappings` carries the external ids for the same title, which is how a
//!   Kitsu-only row gets joined to an AniList row (and vice versa) instead of
//!   the old negative-id trick.

use serde::{Deserialize, Serialize};

pub const API: &str = "https://kitsu.app/api/edge";

#[derive(Debug, Deserialize)]
pub struct Page<T> {
    pub data: Vec<Resource<T>>,
    #[serde(default)]
    pub included: Vec<Included>,
    pub meta: Meta,
}

#[derive(Debug, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub count: u64,
}

#[derive(Debug, Deserialize)]
pub struct Resource<T> {
    pub id: String,
    pub attributes: T,
    #[serde(default)]
    pub relationships: Relationships,
}

#[derive(Debug, Default, Deserialize)]
pub struct Relationships {
    #[serde(default, rename = "mappings")]
    pub mappings: MappingList,
}

#[derive(Debug, Default, Deserialize)]
pub struct MappingList {
    #[serde(default)]
    pub data: Vec<Link>,
}

#[derive(Debug, Deserialize)]
pub struct Link {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct Included {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    pub attributes: IncludedAttrs,
}

#[derive(Debug, Deserialize)]
pub struct IncludedAttrs {
    #[serde(rename = "externalSite", default)]
    pub external_site: String,
    #[serde(rename = "externalId", default)]
    pub external_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Anime {
    #[serde(rename = "canonicalTitle")]
    pub canonical_title: Option<String>,
    #[serde(rename = "titles")]
    pub titles: Titles,
    #[serde(rename = "synopsis")]
    pub synopsis: Option<String>,
    #[serde(rename = "description")]
    pub description: Option<String>,
    /// Percentage string such as "82.27". Already on AniList's 0..100 scale.
    #[serde(rename = "averageRating")]
    pub average_rating: Option<String>,
    #[serde(rename = "userCount")]
    pub user_count: Option<i64>,
    #[serde(rename = "favoritesCount")]
    pub favorites_count: Option<i64>,
    #[serde(rename = "popularityRank")]
    pub popularity_rank: Option<i64>,
    #[serde(rename = "ratingRank")]
    pub rating_rank: Option<i64>,
    #[serde(rename = "ageRating")]
    pub age_rating: Option<String>,
    #[serde(rename = "ageRatingGuide")]
    pub age_rating_guide: Option<String>,
    /// TV / Movie / OVA / ONA / Special
    #[serde(rename = "subtype")]
    pub subtype: Option<String>,
    /// finished / current / upcoming / on_hiatus / unknown
    #[serde(rename = "status")]
    pub status: Option<String>,
    #[serde(rename = "startDate")]
    pub start_date: Option<String>,
    #[serde(rename = "endDate")]
    pub end_date: Option<String>,
    #[serde(rename = "episodeCount")]
    pub episode_count: Option<i64>,
    #[serde(rename = "episodeLength")]
    pub episode_length: Option<i64>,
    #[serde(rename = "totalLength")]
    pub total_length: Option<i64>,
    #[serde(rename = "youtubeVideoId")]
    pub youtube_video_id: Option<String>,
    #[serde(rename = "nsfw")]
    pub nsfw: Option<bool>,
    #[serde(rename = "slug")]
    pub slug: Option<String>,
    #[serde(rename = "posterImage")]
    pub poster_image: ImageSet,
    #[serde(rename = "coverImage")]
    pub cover_image: Option<ImageSet>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImageSet {
    #[serde(rename = "tiny", default, skip_serializing_if = "Option::is_none")]
    pub tiny: Option<String>,
    #[serde(rename = "small", default, skip_serializing_if = "Option::is_none")]
    pub small: Option<String>,
    #[serde(rename = "medium", default, skip_serializing_if = "Option::is_none")]
    pub medium: Option<String>,
    #[serde(rename = "large", default, skip_serializing_if = "Option::is_none")]
    pub large: Option<String>,
    #[serde(rename = "original", default, skip_serializing_if = "Option::is_none")]
    pub original: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Titles {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub en: Option<String>,
    #[serde(rename = "en_jp", default, skip_serializing_if = "Option::is_none")]
    pub en_jp: Option<String>,
    #[serde(rename = "en_us", default, skip_serializing_if = "Option::is_none")]
    pub en_us: Option<String>,
    #[serde(rename = "en_gb", default, skip_serializing_if = "Option::is_none")]
    pub en_gb: Option<String>,
    #[serde(rename = "ja_jp", default, skip_serializing_if = "Option::is_none")]
    pub ja_jp: Option<String>,
    #[serde(rename = "pt_br", default, skip_serializing_if = "Option::is_none")]
    pub pt_br: Option<String>,
}

/// External ids of a Kitsu entry, resolved from `included` mappings.
#[derive(Debug, Default, Clone)]
pub struct ExternalIds {
    pub anilist_id: Option<i64>,
    pub mal_id: Option<i64>,
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<String>,
}

pub fn resolve_external_ids(page: &Page<Anime>) -> Vec<ExternalIds> {
    let by_id: std::collections::HashMap<&str, &IncludedAttrs> = page
        .included
        .iter()
        .filter(|i| i.kind == "mappings")
        .map(|i| (i.id.as_str(), &i.attributes))
        .collect();

    page.data
        .iter()
        .map(|res| {
            let mut out = ExternalIds::default();
            for link in &res.relationships.mappings.data {
                let Some(attrs) = by_id.get(link.id.as_str()) else {
                    continue;
                };
                let value = attrs.external_id.trim();
                if value.is_empty() {
                    continue;
                }
                match attrs.external_site.as_str() {
                    "anilist" => out.anilist_id = value.parse().ok(),
                    "myanimelist/anime" | "myanimelist" => out.mal_id = value.parse().ok(),
                    "imdb" => out.imdb_id = Some(value.to_string()),
                    "themoviedb/tv" | "themoviedb/movie" => out.tmdb_id = Some(value.to_string()),
                    _ => {}
                }
            }
            out
        })
        .collect()
}

/// Sorts accepted by Kitsu. Verified against the live API; several intuitive
/// names (`title`, `popularity`) return HTTP 400 and are deliberately absent.
pub const SORTS: &[&str] = &[
    "-popularityRank",
    "popularityRank",
    "-ratingRank",
    "ratingRank",
    "-favoritesCount",
    "favoritesCount",
    "-userCount",
    "userCount",
    "averageRating",
    "-averageRating",
    "-createdAt",
    "-startDate",
    "subtype",
];

pub fn page_url(page: u32, per_page: u32, sort: &str, include_mappings: bool) -> String {
    let mut url = format!(
        "{}/anime?page%5Blimit%5D={}&page%5Boffset%5D={}",
        API,
        per_page,
        (page - 1) * per_page
    );
    if SORTS.contains(&sort) {
        url.push_str(&format!("&sort={}", sort));
    }
    if include_mappings {
        url.push_str("&include=mappings");
    }
    url
}

/// Fetches a page of the catalogue.
pub async fn fetch_page(
    up: &crate::upstream::Upstream,
    page: u32,
    per_page: u32,
    sort: &str,
) -> Result<(Page<Anime>, Vec<ExternalIds>, u64), String> {
    let url = page_url(page, per_page, sort, true);
    let value = up.get_json(&url).await?;
    let parsed: Page<Anime> = serde_json::from_value(value)
        .map_err(|e| format!("kitsu: не разобрался ответ: {}", e))?;
    let ids = resolve_external_ids(&parsed);
    let total = parsed.meta.count;
    Ok((parsed, ids, total))
}

/// Characters and voice actors for one entry.
pub async fn fetch_characters(
    up: &crate::upstream::Upstream,
    kitsu_id: &str,
) -> Result<serde_json::Value, String> {
    let url = format!(
        "{}/anime/{}/characters?page%5Blimit%5D=200&include=character,voiceActors",
        API, kitsu_id
    );
    up.get_json(&url).await
}

/// Staff for one entry.
pub async fn fetch_staff(up: &crate::upstream::Upstream, kitsu_id: &str) -> Result<serde_json::Value, String> {
    let url = format!("{}/anime/{}/staff?page%5Blimit%5D=200&include=person", API, kitsu_id);
    up.get_json(&url).await
}

/// Genres and categories, cached once per process.
pub async fn fetch_genres(up: &crate::upstream::Upstream) -> Result<serde_json::Value, String> {
    up.get_json(&format!("{}/genres?page%5Blimit%5D=200", API)).await
}
