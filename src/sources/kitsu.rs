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
    /// `titles` and `posterImage` are `default`ed: both are always present in a
    /// real Kitsu payload, but a long-tail record that lacks either must not
    /// cost the whole page — `fetch_page` parses the page as one value, so a
    /// single required field is a single missing field away from failing an
    /// entire 20-row import.
    #[serde(rename = "titles", default)]
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
    #[serde(rename = "posterImage", default)]
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

/// Genres and categories: a reference document, fetched once per process.
///
/// The list is a fixed taxonomy that Kitsu edits in rare, large batches, and
/// it is asked for at the start of every pass. Reused for the life of the
/// process; a restart is how a deployment picks up an edited list, which is
/// cheaper than asking again on every refresh.
pub async fn fetch_genres(
    up: &crate::upstream::Upstream,
) -> Result<serde_json::Value, String> {
    up.get_json_cached(
        &format!("{}/genres?page%5Blimit%5D=200", API),
        crate::upstream::Freshness::Forever,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A JSON:API page with two entries and the `included` mapping records
    /// that carry their external ids.
    fn page_json() -> serde_json::Value {
        json!({
            "data": [
                {
                    "id": "12",
                    "type": "anime",
                    "attributes": {
                        "canonicalTitle": "Cowboy Bebop",
                        "titles": {
                            "en_jp": "Cowboy Bebop",
                            "en": "Cowboy Bebop",
                            "ja_jp": "カウボーイビバップ"
                        },
                        "synopsis": "<p>A bounty hunter crew.</p>",
                        "averageRating": "82.27",
                        "userCount": 500000,
                        "favoritesCount": 20000,
                        "popularityRank": 42,
                        "ageRating": "R17+",
                        "subtype": "TV",
                        "status": "finished",
                        "startDate": "1998-04-03",
                        "episodeCount": 26,
                        "episodeLength": 24,
                        "youtubeVideoId": "xyz",
                        "nsfw": false,
                        "slug": "cowboy-bebop",
                        "posterImage": {
                            "tiny": "t.jpg", "small": "s.jpg", "medium": "m.jpg",
                            "large": "l.jpg", "original": "o.jpg"
                        },
                        "coverImage": { "large": "cover.jpg" }
                    },
                    "relationships": { "mappings": { "data": [
                        { "id": "map-1" }, { "id": "map-2" }, { "id": "map-3" },
                        { "id": "map-4" }, { "id": "map-missing" }
                    ]}}
                },
                {
                    "id": "13",
                    "type": "anime",
                    "attributes": { "titles": { "en_jp": "Unmapped" } }
                }
            ],
            "included": [
                { "type": "mappings", "id": "map-1", "attributes": {
                    "externalSite": "anilist", "externalId": "1" }},
                { "type": "mappings", "id": "map-2", "attributes": {
                    "externalSite": "myanimelist/anime", "externalId": "1" }},
                { "type": "mappings", "id": "map-3", "attributes": {
                    "externalSite": "imdb", "externalId": "tt0102519" }},
                { "type": "mappings", "id": "map-4", "attributes": {
                    "externalSite": "anilist", "externalId": "" }}
            ],
            "meta": { "count": 20000 }
        })
    }

    // ------------------------------------------------------------ mapping

    #[test]
    fn maps_a_kitsu_page_onto_the_internal_type() {
        let p: Page<Anime> = serde_json::from_value(page_json()).unwrap();
        assert_eq!(p.data.len(), 2);
        assert_eq!(p.meta.count, 20000);

        let a = &p.data[0].attributes;
        assert_eq!(a.canonical_title.as_deref(), Some("Cowboy Bebop"));
        assert_eq!(a.titles.en_jp.as_deref(), Some("Cowboy Bebop"));
        assert_eq!(a.titles.ja_jp.as_deref(), Some("カウボーイビバップ"));
        assert_eq!(a.titles.en_us, None);
        assert_eq!(a.average_rating.as_deref(), Some("82.27"));
        assert_eq!(a.user_count, Some(500_000));
        assert_eq!(a.favorites_count, Some(20_000));
        assert_eq!(a.popularity_rank, Some(42));
        assert_eq!(a.age_rating.as_deref(), Some("R17+"));
        assert_eq!(a.subtype.as_deref(), Some("TV"));
        assert_eq!(a.episode_count, Some(26));
        assert_eq!(a.episode_length, Some(24));
        assert_eq!(a.youtube_video_id.as_deref(), Some("xyz"));
        assert_eq!(a.nsfw, Some(false));
        assert_eq!(a.poster_image.large.as_deref(), Some("l.jpg"));
        assert_eq!(
            a.cover_image.as_ref().and_then(|c| c.large.as_deref()),
            Some("cover.jpg")
        );
    }

    #[test]
    fn a_sparse_kitsu_entry_parses_with_defaults() {
        // Long-tail entries carry almost nothing; the row must still import.
        let p: Page<Anime> = serde_json::from_value(page_json()).unwrap();
        let a = &p.data[1].attributes;
        assert_eq!(a.titles.en_jp.as_deref(), Some("Unmapped"));
        assert_eq!(a.canonical_title, None);
        assert_eq!(a.synopsis, None);
        assert_eq!(a.average_rating, None);
        assert_eq!(a.poster_image.large, None);
        assert!(a.cover_image.is_none());
        assert_eq!(a.nsfw, None);
    }

    #[test]
    fn a_resource_without_relationships_still_parses() {
        // `relationships` is not always present; the default has to keep the
        // field usable without an Option at every call site.
        let v = json!({ "data": [{ "id": "1", "attributes": {} }], "meta": { "count": 0 } });
        let p: Page<Anime> = serde_json::from_value(v).unwrap();
        assert!(p.data[0].relationships.mappings.data.is_empty());
    }

    // ------------------------------------------------------- external ids

    #[test]
    fn resolves_external_ids_from_the_included_mappings() {
        let p: Page<Anime> = serde_json::from_value(page_json()).unwrap();
        let ids = resolve_external_ids(&p);
        assert_eq!(ids.len(), 2);
        // This is the join between Kitsu and AniList: without it every Kitsu
        // entry would become its own duplicate catalogue row.
        assert_eq!(ids[0].anilist_id, Some(1));
        assert_eq!(ids[0].mal_id, Some(1));
        assert_eq!(ids[0].imdb_id.as_deref(), Some("tt0102519"));
        assert_eq!(ids[0].tmdb_id, None);
    }

    #[test]
    fn an_entry_without_mappings_resolves_to_nothing() {
        let p: Page<Anime> = serde_json::from_value(page_json()).unwrap();
        let ids = resolve_external_ids(&p);
        assert_eq!(ids[1].anilist_id, None);
        assert_eq!(ids[1].mal_id, None);
        assert_eq!(ids[1].imdb_id, None);
    }

    #[test]
    fn a_dangling_mapping_link_is_skipped() {
        // `map-missing` is referenced but not included; a naive lookup would
        // panic on the unwrap, and an id invented here would attach a Kitsu row
        // to the wrong AniList title.
        let p: Page<Anime> = serde_json::from_value(page_json()).unwrap();
        let ids = resolve_external_ids(&p);
        // The dangling link is simply absent, and the empty map-4 does not
        // overwrite the anilist id found earlier.
        assert_eq!(ids[0].anilist_id, Some(1));
    }

    #[test]
    fn a_non_numeric_anilist_id_yields_none_instead_of_zero() {
        // `.parse().ok()` rather than `unwrap_or(0)`: a zero would be a real
        // AniList id and would silently attach the row to the wrong title.
        let v = json!({
            "data": [{
                "id": "1", "attributes": {},
                "relationships": { "mappings": { "data": [{ "id": "m" }]}}
            }],
            "included": [{
                "type": "mappings", "id": "m",
                "attributes": { "externalSite": "anilist", "externalId": "not-a-number" }
            }],
            "meta": { "count": 1 }
        });
        let p: Page<Anime> = serde_json::from_value(v).unwrap();
        let ids = resolve_external_ids(&p);
        assert_eq!(ids[0].anilist_id, None);
        assert_eq!(ids[0].mal_id, None);
    }

    #[test]
    fn an_empty_external_id_is_ignored() {
        let v = json!({
            "data": [{
                "id": "1", "attributes": {},
                "relationships": { "mappings": { "data": [{ "id": "m" }]}}
            }],
            "included": [{
                "type": "mappings", "id": "m",
                "attributes": { "externalSite": "anilist", "externalId": "   " }
            }],
            "meta": { "count": 1 }
        });
        let p: Page<Anime> = serde_json::from_value(v).unwrap();
        assert_eq!(resolve_external_ids(&p)[0].anilist_id, None);
    }

    #[test]
    fn an_unknown_external_site_is_ignored() {
        let v = json!({
            "data": [{
                "id": "1", "attributes": {},
                "relationships": { "mappings": { "data": [{ "id": "m" }]}}
            }],
            "included": [{
                "type": "mappings", "id": "m",
                "attributes": { "externalSite": "somebook", "externalId": "123" }
            }],
            "meta": { "count": 1 }
        });
        let p: Page<Anime> = serde_json::from_value(v).unwrap();
        assert_eq!(resolve_external_ids(&p)[0].anilist_id, None);
    }

    #[test]
    fn both_myanimelist_spellings_are_accepted() {
        // Kitsu uses "myanimelist/anime" on some records and "myanimelist" on
        // others; missing one loses the MAL id for a slice of the catalogue.
        for site in ["myanimelist/anime", "myanimelist"] {
            let v = json!({
                "data": [{
                    "id": "1", "attributes": {},
                    "relationships": { "mappings": { "data": [{ "id": "m" }]}}
                }],
                "included": [{
                    "type": "mappings", "id": "m",
                    "attributes": { "externalSite": site, "externalId": "42" }
                }],
                "meta": { "count": 1 }
            });
            let p: Page<Anime> = serde_json::from_value(v).unwrap();
            assert_eq!(resolve_external_ids(&p)[0].mal_id, Some(42), "site {}", site);
        }
    }

    #[test]
    fn both_tmdb_dimensional_variants_are_accepted() {
        for site in ["themoviedb/tv", "themoviedb/movie"] {
            let v = json!({
                "data": [{
                    "id": "1", "attributes": {},
                    "relationships": { "mappings": { "data": [{ "id": "m" }]}}
                }],
                "included": [{
                    "type": "mappings", "id": "m",
                    "attributes": { "externalSite": site, "externalId": "999" }
                }],
                "meta": { "count": 1 }
            });
            let p: Page<Anime> = serde_json::from_value(v).unwrap();
            assert_eq!(
                resolve_external_ids(&p)[0].tmdb_id.as_deref(),
                Some("999"),
                "site {}",
                site
            );
        }
    }

    #[test]
    fn included_records_of_another_type_do_not_leak_into_the_lookup() {
        // The same page includes characters and people; only `mappings` may be
        // used to resolve external ids.
        let v = json!({
            "data": [{
                "id": "1", "attributes": {},
                "relationships": { "mappings": { "data": [{ "id": "m" }]}}
            }],
            "included": [{
                "type": "anime", "id": "m",
                "attributes": { "externalSite": "anilist", "externalId": "7" }
            }],
            "meta": { "count": 1 }
        });
        let p: Page<Anime> = serde_json::from_value(v).unwrap();
        assert_eq!(resolve_external_ids(&p)[0].anilist_id, None);
    }

    #[test]
    fn a_page_with_no_meta_still_parses() {
        // `meta` is required by the struct, so a source that omits it must fail
        // the page loudly — the loader counts on it for pagination.
        let v = json!({ "data": [] });
        assert!(serde_json::from_value::<Page<Anime>>(v).is_err());
    }

    // ---------------------------------------------------------- url build

    #[test]
    fn page_url_offsets_by_page() {
        let url = page_url(1, 20, "-popularityRank", true);
        assert!(url.contains("page%5Blimit%5D=20"));
        assert!(url.contains("page%5Boffset%5D=0"));
        assert!(url.contains("sort=-popularityRank"));
        assert!(url.contains("include=mappings"));

        let url = page_url(3, 20, "userCount", false);
        assert!(url.contains("page%5Boffset%5D=40"));
        assert!(!url.contains("include="));
    }

    #[test]
    fn page_url_drops_a_sort_kitsu_would_reject() {
        // "title" and "popularity" are not valid Kitsu sorts and come back as
        // HTTP 400, so they must not be sent.
        let url = page_url(1, 20, "title", false);
        assert!(!url.contains("sort="));
    }

    #[test]
    fn every_listed_sort_is_accepted_by_the_url_builder() {
        for sort in SORTS {
            assert!(
                page_url(1, 20, sort, false).contains("sort="),
                "sort {} не подставляется",
                sort
            );
        }
    }
}
