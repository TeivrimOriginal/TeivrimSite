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
    /// `default`ed on purpose: a page is parsed as one value, so a single
    /// record without a title object would otherwise cost the whole page of 50
    /// rows. `Title` is all-optional, so the default is an empty one and the
    /// row imports with no names.
    #[serde(default)]
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


#[derive(Debug)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// One AniList page as the API actually returns it, with every field the
    /// loader reads filled in. Written as JSON rather than a `Media` literal
    /// on purpose: the point is to test the *mapping*, and a literal would
    /// only re-test serde against itself.
    fn full_page() -> Value {
        json!({
            "data": {
                "Page": {
                    "pageInfo": { "hasNextPage": true, "total": 42 },
                    "media": [{
                        "id": 16498,
                        "idMal": 20,
                        "title": {
                            "romaji": "Shingeki no Kyojin",
                            "english": "Attack on Titan",
                            "native": "進撃の巨人",
                            "userPreferred": "Shingeki no Kyojin"
                        },
                        "synonyms": ["Shingeki no Kyojin Shou", "AoT"],
                        "format": "TV",
                        "status": "FINISHED",
                        "description": "<p>Huge humanoids</p><p>Eat people</p>",
                        "duration": 24,
                        "episodes": 25,
                        "chapters": null,
                        "volumes": null,
                        "countryOfOrigin": "JP",
                        "isAdult": false,
                        "isLicensed": true,
                        "startDate": { "year": 2013, "month": 4, "day": 7 },
                        "endDate": { "year": 2023, "month": 11, "day": 4 },
                        "season": "SPRING",
                        "seasonYear": 2013,
                        "averageScore": 84,
                        "meanScore": 85,
                        "popularity": 12345,
                        "favourites": 6789,
                        "trending": 42,
                        "coverImage": {
                            "extraLarge": "https://s4.anilist.co/xl.jpg",
                            "large": "https://s4.anilist.co/l.jpg",
                            "medium": "https://s4.anilist.co/m.jpg",
                            "color": "#8f9494"
                        },
                        "bannerImage": "https://s4.anilist.co/banner.jpg",
                        "trailer": {
                            "id": "abc123",
                            "site": "youtube",
                            "thumbnail": "https://img.anili.st/thumb.jpg"
                        },
                        "genres": ["Action", "Drama"],
                        "tags": [
                            { "name": "Male Protagonist", "rank": 90, "isMediaSpoiler": false },
                            { "name": "Survival", "rank": 80, "isMediaSpoiler": true }
                        ],
                        "studios": { "nodes": [
                            { "name": "Wit Studio", "isAnimationStudio": true }
                        ]},
                        "relations": { "edges": [
                            {
                                "relationType": "PREQUEL",
                                "node": {
                                    "id": 11061,
                                    "title": { "romaji": "Shingeki no Kyojin: Kaban", "english": null, "native": null },
                                    "format": "MOVIE",
                                    "status": "FINISHED",
                                    "coverImage": { "large": "https://s4.anilist.co/kaban.jpg" }
                                }
                            }
                        ]},
                        "externalLinks": [
                            { "id": 1, "url": "https://myanimelist.net/anime/20", "site": "MAL", "type": "ANILIST_SITE" }
                        ],
                        "streamingEpisodes": [
                            { "title": "Episode 1", "thumbnail": null, "url": "https://anilist.co/watch/1", "site": "anilist" }
                        ],
                        "recommendations": { "nodes": [
                            {
                                "rating": 95,
                                "mediaRecommendation": {
                                    "id": 127230,
                                    "title": { "romaji": "Gingitsune" },
                                    "format": "TV_SHORT",
                                    "coverImage": { "large": "https://s4.anilist.co/gingitsune.jpg" }
                                }
                            }
                        ]}
                    }]
                }
            }
        })
    }

    // ------------------------------------------------------------- mapping

    #[test]
    fn maps_a_full_anilist_page_onto_the_internal_media_type() {
        let f = parse_page(full_page()).unwrap();
        assert!(f.has_next);
        assert_eq!(f.media.len(), 1);
        let m = &f.media[0];

        assert_eq!(m.id, 16498);
        assert_eq!(m.id_mal, Some(20));
        assert_eq!(m.title.romaji.as_deref(), Some("Shingeki no Kyojin"));
        assert_eq!(m.title.english.as_deref(), Some("Attack on Titan"));
        assert_eq!(m.title.native.as_deref(), Some("進撃の巨人"));
        assert_eq!(m.format.as_deref(), Some("TV"));
        assert_eq!(m.status.as_deref(), Some("FINISHED"));
        assert_eq!(m.duration, Some(24));
        assert_eq!(m.episodes, Some(25));
        // A manga-only field is explicitly null upstream; it must stay None
        // rather than becoming 0, which would read as "zero chapters".
        assert_eq!(m.chapters, None);
        assert_eq!(m.country_of_origin.as_deref(), Some("JP"));
        assert_eq!(m.is_adult, Some(false));
        assert_eq!(m.is_licensed, Some(true));
        assert_eq!(m.average_score, Some(84));
        assert_eq!(m.season_year, Some(2013));
    }

    #[test]
    fn maps_camel_case_field_names() {
        // AniList's wire names are camelCase; the rust struct is snake_case.
        // A rename typo here is invisible until the column is silently NULL
        // for every row in the catalogue.
        let m = &parse_page(full_page()).unwrap().media[0];
        assert_eq!(m.id_mal, Some(20));
        assert_eq!(m.country_of_origin.as_deref(), Some("JP"));
        assert_eq!(m.is_adult, Some(false));
        assert_eq!(m.is_licensed, Some(true));
        assert_eq!(m.season_year, Some(2013));
        assert_eq!(m.average_score, Some(84));
        assert_eq!(m.mean_score, Some(85));
        assert_eq!(
            m.cover_image.as_ref().and_then(|c| c.extra_large.as_deref()),
            Some("https://s4.anilist.co/xl.jpg")
        );
        assert_eq!(m.banner_image.as_deref(), Some("https://s4.anilist.co/banner.jpg"));
        assert_eq!(
            m.start_date.as_ref().and_then(|d| d.to_iso()).as_deref(),
            Some("2013-04-07")
        );
    }

    #[test]
    fn maps_the_nested_connections() {
        let m = &parse_page(full_page()).unwrap().media[0];

        let studios = m.studios.as_ref().and_then(|s| s.nodes.as_ref()).unwrap();
        assert_eq!(studios.len(), 1);
        assert_eq!(studios[0].name.as_deref(), Some("Wit Studio"));
        assert_eq!(studios[0].is_animation_studio, Some(true));

        let edges = m
            .relations
            .as_ref()
            .and_then(|r| r.edges.as_ref())
            .unwrap();
        assert_eq!(edges[0].relation_type.as_deref(), Some("PREQUEL"));
        let node = edges[0].node.as_ref().unwrap();
        assert_eq!(node.id, 11061);
        assert_eq!(
            node.title.as_ref().and_then(|t| t.romaji.as_deref()),
            Some("Shingeki no Kyojin: Kaban")
        );
        assert_eq!(node.title.as_ref().and_then(|t| t.english.as_ref()), None);
        assert_eq!(node.format.as_deref(), Some("MOVIE"));

        let recs = m
            .recommendations
            .as_ref()
            .and_then(|r| r.nodes.as_ref())
            .unwrap();
        assert_eq!(recs[0].rating, Some(95));
        let rec = recs[0].media_recommendation.as_ref().unwrap();
        assert_eq!(rec.id, 127230);

        let tags = m.tags.as_ref().unwrap();
        assert_eq!(tags[0].rank, Some(90));
        assert_eq!(tags[0].is_media_spoiler, Some(false));
        assert_eq!(tags[1].is_media_spoiler, Some(true));
    }

    // ------------------------------------------------- optional / defaults

    #[test]
    fn a_minimal_media_entry_parses_with_everything_else_none() {
        // AniList is not obliged to fill anything in; the minimum is an id and
        // a title, and that row must still import instead of failing the page.
        let v = json!({
            "data": { "Page": { "pageInfo": { "hasNextPage": false }, "media": [
                { "id": 1, "title": { "romaji": "Only Title" } }
            ]}}
        });
        let f = parse_page(v).unwrap();
        assert!(!f.has_next);
        let m = &f.media[0];
        assert_eq!(m.id, 1);
        assert_eq!(m.title.romaji.as_deref(), Some("Only Title"));
        assert_eq!(m.title.english, None);
        assert_eq!(m.id_mal, None);
        assert!(m.cover_image.is_none());
        assert!(m.trailer.is_none());
        assert_eq!(m.genres, None);
        assert!(m.tags.is_none());
        assert!(m.studios.is_none());
        assert!(m.relations.is_none());
        assert!(m.external_links.is_none());
        assert!(m.streaming_episodes.is_none());
        assert!(m.recommendations.is_none());
        assert_eq!(m.synonyms, None);
        assert_eq!(m.is_adult, None);
    }

    #[test]
    fn connections_with_a_null_node_list_parse_to_none() {
        // AniList sends `"studios": null` for unknown entries, which is
        // different from `"studios": {"nodes": []}`.
        let v = json!({
            "data": { "Page": { "pageInfo": { "hasNextPage": false }, "media": [
                {
                    "id": 1,
                    "title": {},
                    "studios": { "nodes": null },
                    "recommendations": { "nodes": null },
                    "relations": { "edges": null }
                }
            ]}}
        });
        let m = &parse_page(v).unwrap().media[0];
        assert!(m.studios.as_ref().and_then(|s| s.nodes.as_ref()).is_none());
        assert!(m.recommendations.as_ref().and_then(|r| r.nodes.as_ref()).is_none());
        assert!(m.relations.as_ref().and_then(|r| r.edges.as_ref()).is_none());
    }

    #[test]
    fn a_relation_edge_with_a_null_node_does_not_panic() {
        // A deleted or hidden target makes AniList emit `"node": null`. The
        // loader filter_maps over it, so it must be skipped rather than
        // dereferenced.
        let v = json!({
            "data": { "Page": { "pageInfo": { "hasNextPage": false }, "media": [
                { "id": 1, "title": {}, "relations": { "edges": [
                    { "relationType": "PREQUEL", "node": null },
                    { "relationType": "SEQUEL" }
                ]}}
            ]}}
        });
        let m = &parse_page(v).unwrap().media[0];
        let edges = m.relations.as_ref().and_then(|r| r.edges.as_ref()).unwrap();
        assert_eq!(edges.len(), 2);
        assert!(edges[0].node.is_none());
        assert_eq!(edges[1].relation_type.as_deref(), Some("SEQUEL"));
    }

    #[test]
    fn an_empty_title_object_is_accepted() {
        // A row can reach the catalogue with no title at all; the API layer
        // substitutes "Без названия" rather than dropping the row.
        let v = json!({
            "data": { "Page": { "pageInfo": { "hasNextPage": false }, "media": [
                { "id": 1 }
            ]}}
        });
        let m = &parse_page(v).unwrap().media[0];
        assert!(m.title.romaji.is_none());
    }

    // -------------------------------------------------------------- errors

    #[test]
    fn a_graphql_error_becomes_an_err_not_an_empty_page() {
        // A rate-limit or a bad sort comes back as HTTP 200 with `errors`, which
        // is exactly the case that used to look like "the last page".
        let v = json!({
            "data": null,
            "errors": [{ "message": "Too Many Requests" }]
        });
        let e = parse_page(v).unwrap_err();
        assert!(e.contains("Too Many Requests"), "got {:?}", e);
    }

    #[test]
    fn a_null_data_block_is_an_error() {
        let v = json!({ "data": null, "errors": [] });
        assert!(parse_page(v).is_err());
    }

    #[test]
    fn a_missing_data_block_is_an_error() {
        let v = json!({ "errors": [] });
        assert!(parse_page(v).is_err());
    }

    #[test]
    fn a_body_that_is_not_the_expected_shape_is_an_error() {
        // HTML from a captive portal, a truncated body, a proxy error page:
        // all of them must fail the page loudly instead of panicking.
        let v = json!({ "unexpected": true });
        assert!(parse_page(v).is_err());
    }

    #[test]
    fn a_page_with_no_media_is_empty_and_finished() {
        let v = json!({
            "data": { "Page": { "pageInfo": { "hasNextPage": false }, "media": [] }}
        });
        let f = parse_page(v).unwrap();
        assert!(f.media.is_empty());
        assert!(!f.has_next);
    }

    // --------------------------------------------------------- fuzzy dates

    #[test]
    fn fuzzy_date_renders_the_longest_known_prefix() {
        // AniList models a partial date as independent nullable fields, so
        // three shapes are possible and all three occur in the catalogue.
        let full = FuzzyDate { year: Some(2024), month: Some(7), day: Some(14) };
        assert_eq!(full.to_iso().as_deref(), Some("2024-07-14"));

        let no_day = FuzzyDate { year: Some(2024), month: Some(7), day: None };
        assert_eq!(no_day.to_iso().as_deref(), Some("2024-07"));

        let year_only = FuzzyDate { year: Some(2024), month: None, day: None };
        assert_eq!(year_only.to_iso().as_deref(), Some("2024"));
    }

    #[test]
    fn fuzzy_date_without_a_year_has_no_iso_form() {
        // An unknown year is the one case that cannot be rendered at all, and
        // a fabricated "0000" would break every year range filter.
        let d = FuzzyDate { year: None, month: Some(7), day: Some(14) };
        assert_eq!(d.to_iso(), None);
    }

    #[test]
    fn fuzzy_date_keeps_the_order_of_a_partial_date() {
        // Year + day but no month is nonsense input; it must degrade to the
        // year rather than inventing a month.
        let d = FuzzyDate { year: Some(2024), month: None, day: Some(14) };
        assert_eq!(d.to_iso().as_deref(), Some("2024"));
    }

    // ------------------------------------------------------ query builder

    #[test]
    fn build_query_interpolates_the_paging_arguments() {
        let q = build_query(3, 50, "POPULARITY_DESC");
        assert!(q.contains("Page(page: 3, perPage: 50)"));
        assert!(q.contains("sort: [POPULARITY_DESC]"));
    }

    #[test]
    fn build_query_falls_back_on_an_unknown_sort() {
        // The sort is interpolated into the query body, so an unrecognised
        // value must never reach the string.
        let q = build_query(1, 10, "'; DROP TABLE Page; --");
        assert!(q.contains("sort: [POPULARITY_DESC]"));
        assert!(!q.contains("DROP TABLE"));
    }

    #[test]
    fn build_query_accepts_every_documented_sort() {
        for sort in SORTS {
            let q = build_query(1, 50, sort);
            assert!(q.contains(&format!("sort: [{}]", sort)), "sort {}", sort);
        }
    }

    #[test]
    fn build_query_asks_for_the_fields_the_loader_maps() {
        // Dropping a field from MEDIA_FIELDS silently empties a section of the
        // detail page, so the contract is pinned here.
        let q = build_query(1, 50, "ID");
        for field in [
            "idMal", "countryOfOrigin", "isAdult", "seasonYear", "averageScore",
            "coverImage", "bannerImage", "streamingEpisodes", "externalLinks",
            "recommendations", "relations", "studios(isMain: true)", "genres",
        ] {
            assert!(q.contains(field), "запрос не запрашивает {}", field);
        }
    }
}

