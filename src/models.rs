//! Wire types for the JSON API.
//!
//! Field names are camelCase and deliberately explicit. The v1 detail endpoint
//! returned a raw `SELECT *`, which meant the client had to know that a column
//! called `genres_json` held parsed JSON — and the frontend looked for
//! `genres`, so every list on the detail page silently rendered empty. A
//! hand-written struct makes the contract something a compiler checks.

use serde::{Deserialize, Serialize};

// ==================================================================
// Requests
// ==================================================================

/// Every filter the catalogue list accepts. `serde` defaults everything, so an
/// unknown or absent query parameter is simply "no filter" rather than a 400.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ListQuery {
    pub page: Option<u32>,
    pub per_page: Option<i64>,
    pub q: Option<String>,
    pub sort: Option<String>,
    pub format: Option<String>,
    pub status: Option<String>,
    pub season: Option<String>,
    pub season_year: Option<i64>,
    pub genre: Option<String>,
    pub tag: Option<String>,
    pub studio: Option<String>,
    pub country: Option<String>,
    pub year: Option<String>,
    pub year_from: Option<i64>,
    pub year_to: Option<i64>,
    pub score_from: Option<i64>,
    pub score_to: Option<i64>,
    pub duration_from: Option<i64>,
    pub duration_to: Option<i64>,
    pub episodes_from: Option<i64>,
    pub episodes_to: Option<i64>,
    pub adult: Option<String>,
    pub licensed: Option<String>,
    pub has_russian: Option<String>,
    pub has_trailer: Option<String>,
    /// Restrict to a user's watchlist / favourites. Requires a token.
    pub in_list: Option<String>,
}



// ==================================================================
// Responses
// ==================================================================

/// A row in the catalogue grid. Deliberately small: the grid renders hundreds of
/// these, and v1 shipped every JSON blob column to the client for each one.
#[derive(Debug, Clone, Serialize)]
pub struct AnimeSummary {
    pub uid: String,
    pub title: String,
    pub title_romaji: Option<String>,
    pub title_english: Option<String>,
    pub title_russian: Option<String>,
    pub title_native: Option<String>,
    pub cover: Option<String>,
    pub cover_color: Option<String>,
    pub score: Option<i64>,
    pub score_source: Option<String>,
    pub format: Option<String>,
    pub status: Option<String>,
    pub episodes: Option<i64>,
    pub duration: Option<i64>,
    pub year: Option<i64>,
    pub season: Option<String>,
    pub season_year: Option<i64>,
    pub country: Option<String>,
    pub is_adult: bool,
    pub genres: Vec<Genre>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Paged<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub per_page: i64,
    pub total: i64,
    pub total_pages: i64,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Genre {
    pub id: i64,
    pub slug: String,
    pub name: String,
    pub name_ru: Option<String>,
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i64>,
}

/// One search suggestion.
#[derive(Debug, Clone, Serialize)]
pub struct Suggestion {
    pub uid: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_romaji: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_english: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_russian: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title_native: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub popularity: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NamedRef {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_ru: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_main: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Relation {
    pub relation: String,
    pub uid: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExternalLink {
    pub site: String,
    pub url: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamingLink {
    pub site: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Recommendation {
    pub uid: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Tag {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rank: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spoiler: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Person {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_actor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub positions: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Trailer {
    pub site: String,
    pub id: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceIds {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anilist: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kitsu: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shikimori: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mal: Option<i64>,
}

/// Everything the detail page renders.
#[derive(Debug, Clone, Serialize)]
pub struct AnimeDetail {
    pub uid: String,
    pub ids: SourceIds,

    pub title_romaji: Option<String>,
    pub title_english: Option<String>,
    pub title_native: Option<String>,
    pub title_russian: Option<String>,
    pub synonyms: Vec<String>,

    pub format: Option<String>,
    pub status: Option<String>,
    pub description: Option<String>,
    pub description_ru: Option<String>,

    pub duration: Option<i64>,
    pub episodes: Option<i64>,
    pub chapters: Option<i64>,
    pub volumes: Option<i64>,
    pub country: Option<String>,
    pub is_adult: bool,
    pub is_licensed: Option<bool>,

    pub season: Option<String>,
    pub season_year: Option<i64>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,

    pub score: Option<i64>,
    pub score_source: Option<String>,
    pub mean_score: Option<i64>,
    pub popularity: Option<i64>,
    pub favourites: Option<i64>,
    pub trending: Option<i64>,
    pub rating_count: Option<i64>,

    pub cover_small: Option<String>,
    pub cover_medium: Option<String>,
    pub cover_large: Option<String>,
    pub cover_color: Option<String>,
    pub banner: Option<String>,

    pub trailer: Option<Trailer>,
    pub genres: Vec<Genre>,
    pub tags: Vec<Tag>,
    pub studios: Vec<NamedRef>,
    pub producers: Vec<NamedRef>,
    pub licensors: Vec<NamedRef>,
    pub age_rating: Option<String>,
    pub relations: Vec<Relation>,
    pub external_links: Vec<ExternalLink>,
    pub streaming: Vec<StreamingLink>,
    pub recommendations: Vec<Recommendation>,
    pub characters: Vec<Person>,
    pub staff: Vec<Person>,

    /// Watchlist state of the requesting user, when a token was supplied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library: Option<LibraryEntry>,

    pub updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryEntry {
    pub uid: String,
    pub status: String,
    pub is_favorite: bool,
    pub score: Option<i64>,
    pub progress: Option<i64>,
    pub episodes: Option<i64>,
    pub notes: Option<String>,
    pub updated_at: i64,
}

// ==================================================================
// Auth
// ==================================================================

#[derive(Debug, Deserialize)]
pub struct RegisterBody {
    pub username: String,
    pub email: Option<String>,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginBody {
    /// Username or e-mail.
    pub login: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub expires_at: i64,
    pub user: PublicUser,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicUser {
    pub id: i64,
    pub username: String,
    pub email: Option<String>,
    pub created_at: i64,
}

