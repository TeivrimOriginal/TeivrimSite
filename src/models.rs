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
    /// Alias `has_ru`: the shipped frontend used that name in its filter
    /// state, and an unknown parameter is a 400 here, so a bookmarked URL from
    /// the previous build would otherwise break the catalogue page.
    #[serde(alias = "has_ru")]
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn value<T: Serialize>(v: &T) -> serde_json::Value {
        serde_json::to_value(v).expect("serialize")
    }

    // ------------------------------------------------------------ ListQuery

    #[test]
    fn list_query_accepts_the_documented_filters() {
        // The catalogue URL is the primary interface, so the whole filter set
        // has to survive `serde` under snake_case names.
        let q: ListQuery = serde_json::from_value(json!({
            "page": 2,
            "per_page": 25,
            "q": "titans",
            "sort": "score",
            "format": "TV",
            "status": "RELEASING",
            "season": "SPRING",
            "season_year": 2013,
            "genre": "action,drama",
            "tag": "military",
            "studio": "wit studio",
            "country": "JP",
            "year": "2010-2015",
            "score_from": 50,
            "score_to": 90,
            "duration_from": 20,
            "duration_to": 120,
            "episodes_from": 1,
            "episodes_to": 24,
            "adult": "no",
            "licensed": "yes",
            "has_russian": "yes",
            "has_trailer": "1",
            "in_list": "favorites"
        }))
        .unwrap();
        assert_eq!(q.page, Some(2));
        assert_eq!(q.per_page, Some(25));
        assert_eq!(q.q.as_deref(), Some("titans"));
        assert_eq!(q.sort.as_deref(), Some("score"));
        assert_eq!(q.genre.as_deref(), Some("action,drama"));
        assert_eq!(q.season_year, Some(2013));
        assert_eq!(q.score_to, Some(90));
        assert_eq!(q.episodes_from, Some(1));
        assert_eq!(q.adult.as_deref(), Some("no"));
        assert_eq!(q.has_russian.as_deref(), Some("yes"));
        assert_eq!(q.has_trailer.as_deref(), Some("1"));
        assert_eq!(q.in_list.as_deref(), Some("favorites"));
    }

    #[test]
    fn an_absent_filter_is_none_and_not_an_error() {
        // "no filter" has to be representable: every endpoint reads these
        // fields with `if let Some(..)`, and a default would silently apply a
        // filter nobody asked for.
        let q: ListQuery = serde_json::from_value(json!({})).unwrap();
        assert_eq!(q.page, None);
        assert_eq!(q.per_page, None);
        assert_eq!(q.q, None);
        assert_eq!(q.sort, None);
        assert_eq!(q.genre, None);
        assert_eq!(q.in_list, None);
    }

    #[test]
    fn the_filter_accepts_both_spellings_of_the_russian_title_flag() {
        // The frontend used `has_ru` before the API renamed it, and an unknown
        // parameter is a 400, so a link saved from that build has to keep
        // working.
        let canonical: ListQuery = serde_json::from_value(json!({ "has_russian": "yes" })).unwrap();
        assert_eq!(canonical.has_russian.as_deref(), Some("yes"));
        let legacy: ListQuery = serde_json::from_value(json!({ "has_ru": "yes" })).unwrap();
        assert_eq!(legacy.has_russian.as_deref(), Some("yes"));
    }

    #[test]
    fn an_unknown_filter_is_rejected_rather_than_ignored() {
        // `deny_unknown_fields`: a typo'd filter that was silently dropped made
        // the UI look like the catalogue had lost its data.
        let err = serde_json::from_value::<ListQuery>(json!({ "perpage": 25 }));
        assert!(err.is_err());
    }

    #[test]
    fn a_null_filter_is_none() {
        let q: ListQuery = serde_json::from_value(json!({ "page": null, "q": null })).unwrap();
        assert_eq!(q.page, None);
        assert_eq!(q.q, None);
    }

    #[test]
    fn a_wrongly_typed_filter_is_an_error() {
        // The alternative — a string page number coerced to 0 — turns a client
        // bug into "show me nothing", and 400 is a much better answer.
        assert!(serde_json::from_value::<ListQuery>(json!({ "page": "second" })).is_err());
        assert!(serde_json::from_value::<ListQuery>(json!({ "per_page": "many" })).is_err());
        assert!(serde_json::from_value::<ListQuery>(json!({ "q": 42 })).is_err());
    }

    #[test]
    fn list_query_defaults_to_an_empty_filter() {
        let q = ListQuery::default();
        assert_eq!(q.page, None);
        assert_eq!(q.year, None);
    }

    // -------------------------------------------------------- AnimeSummary

    fn summary() -> AnimeSummary {
        AnimeSummary {
            uid: "al:16498".into(),
            title: "Атака Титанов".into(),
            title_romaji: Some("Shingeki no Kyojin".into()),
            title_english: Some("Attack on Titan".into()),
            title_russian: Some("Атака Титанов".into()),
            title_native: Some("進撃の巨人".into()),
            cover: Some("https://s4.anilist.co/l.jpg".into()),
            cover_color: Some("#8f9494".into()),
            score: Some(84),
            score_source: Some("anilist".into()),
            format: Some("TV".into()),
            status: Some("FINISHED".into()),
            episodes: Some(25),
            duration: Some(24),
            year: Some(2013),
            season: Some("SPRING".into()),
            season_year: Some(2013),
            country: Some("JP".into()),
            is_adult: false,
            genres: vec![Genre {
                id: 1,
                slug: "action".into(),
                name: "Action".into(),
                name_ru: Some("Боевик".into()),
                category: Some("genre".into()),
                count: Some(12),
            }],
        }
    }

    #[test]
    fn a_summary_serialises_with_the_field_names_the_client_reads() {
        // The frontend was written against these exact keys; v2 moved to
        // camelCase and the grid broke when they drifted.
        let v = value(&summary());
        for key in [
            "uid", "title", "title_romaji", "title_english", "title_russian", "title_native",
            "cover", "cover_color", "score", "score_source", "format", "status", "episodes",
            "duration", "year", "season", "season_year", "country", "is_adult", "genres",
        ] {
            assert!(v.get(key).is_some(), "в ответе нет поля {}", key);
        }
        assert_eq!(v["uid"], json!("al:16498"));
        assert_eq!(v["score"], json!(84));
        assert_eq!(v["is_adult"], json!(false));
        assert_eq!(v["genres"][0]["name_ru"], json!("Боевик"));
    }

    #[test]
    fn a_summary_keeps_nulls_visible() {
        // Summary fields are NOT `skip_serializing_if`: the grid has to be able
        // to tell "no cover" from "field missing" without special-casing.
        let mut s = summary();
        s.cover = None;
        s.score = None;
        let v = value(&s);
        assert!(v.get("cover").is_some());
        assert_eq!(v["cover"], json!(null));
        assert_eq!(v["score"], json!(null));
    }

    // ---------------------------------------------------------------- Paged

    #[test]
    fn paged_reports_the_arithmetic_the_grid_needs() {
        let p = Paged {
            items: vec![summary()],
            page: 2,
            per_page: 24,
            total: 100,
            total_pages: 5,
            has_more: true,
        };
        let v = value(&p);
        assert_eq!(v["page"], json!(2));
        assert_eq!(v["per_page"], json!(24));
        assert_eq!(v["total"], json!(100));
        assert_eq!(v["total_pages"], json!(5));
        assert_eq!(v["has_more"], json!(true));
        assert_eq!(v["items"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn an_empty_page_still_carries_its_paging_metadata() {
        // The frontend shows "nothing found" from `total == 0`, so an empty
        // list must not collapse the envelope.
        let p = Paged::<AnimeSummary> {
            items: vec![],
            page: 7,
            per_page: 48,
            total: 0,
            total_pages: 0,
            has_more: false,
        };
        let v = value(&p);
        assert_eq!(v["items"], json!([]));
        assert_eq!(v["page"], json!(7));
        assert_eq!(v["has_more"], json!(false));
    }

    // ----------------------------------------------- optional-field policy

    #[test]
    fn genre_count_is_omitted_when_absent() {
        // Detail-page genres carry no count; the filter list does. One shape
        // with a nullable field would force every client to null-check.
        let g = Genre {
            id: 5,
            slug: "action".into(),
            name: "Action".into(),
            name_ru: None,
            category: Some("genre".into()),
            count: None,
        };
        let v = value(&g);
        assert!(v.get("count").is_none());
        assert!(v.get("name_ru").is_some());
        assert_eq!(v["name_ru"], json!(null));

        let g = Genre { count: Some(42), ..g };
        assert_eq!(value(&g)["count"], json!(42));
    }

    #[test]
    fn suggestion_omits_every_absent_optional() {
        // Type-ahead results are fetched per keystroke, so the response has to
        // stay small.
        let s = Suggestion {
            uid: "al:1".into(),
            title: "One".into(),
            title_romaji: None,
            title_english: None,
            title_russian: None,
            title_native: None,
            cover: None,
            popularity: Some(5),
            score: None,
        };
        let v = value(&s);
        assert_eq!(v.as_object().unwrap().len(), 3);
        assert!(v.get("title_romaji").is_none());
        assert!(v.get("score").is_none());
        assert_eq!(v["popularity"], json!(5));
    }

    #[test]
    fn external_link_renames_kind_to_type() {
        // `kind` is the rust name, `type` is the wire name; the frontend
        // filters on `type`.
        let l = ExternalLink {
            site: "MAL".into(),
            url: "https://myanimelist.net/anime/20".into(),
            kind: Some("ANILIST_SITE".into()),
        };
        let v = value(&l);
        assert_eq!(v["type"], json!("ANILIST_SITE"));
        assert!(v.get("kind").is_none());
    }

    #[test]
    fn named_ref_omits_absent_optionals() {
        let n = NamedRef {
            name: "Wit Studio".into(),
            name_ru: Some("Wit Studio".into()),
            is_main: Some(true),
        };
        let v = value(&n);
        assert_eq!(v["is_main"], json!(true));
        assert_eq!(v["name_ru"], json!("Wit Studio"));

        // A studio with no Russian name and no main flag shrinks to just the
        // name, which is what the credits list renders.
        let bare = value(&NamedRef { name: "MAPPA".into(), name_ru: None, is_main: None });
        assert_eq!(bare.as_object().unwrap().len(), 1);
        assert_eq!(bare["name"], json!("MAPPA"));
    }

    // --------------------------------------------------------- AnimeDetail

    #[test]
    fn a_detail_response_omits_an_absent_library_entry() {
        // Anonymous requests have no watchlist state, and an explicit
        // `"library": null` is not the same as "no watchlist entry".
        let d = AnimeDetail {
            uid: "al:1".into(),
            ids: SourceIds { anilist: Some(1), kitsu: None, shikimori: None, mal: Some(20) },
            title_romaji: Some("Shingeki no Kyojin".into()),
            title_english: None,
            title_native: None,
            title_russian: Some("Атака Титанов".into()),
            synonyms: vec!["AoT".into()],
            format: Some("TV".into()),
            status: None,
            description: None,
            description_ru: None,
            duration: Some(24),
            episodes: Some(25),
            chapters: None,
            volumes: None,
            country: Some("JP".into()),
            is_adult: false,
            is_licensed: Some(true),
            season: None,
            season_year: None,
            start_date: Some("2013-04-07".into()),
            end_date: None,
            score: Some(84),
            score_source: Some("anilist".into()),
            mean_score: Some(85),
            popularity: None,
            favourites: None,
            trending: None,
            rating_count: None,
            cover_small: None,
            cover_medium: None,
            cover_large: Some("https://s4.anilist.co/xl.jpg".into()),
            cover_color: None,
            banner: None,
            trailer: Some(Trailer {
                site: "youtube".into(),
                id: "abc".into(),
                url: "https://www.youtube.com/watch?v=abc".into(),
                thumbnail: None,
            }),
            genres: vec![],
            tags: vec![Tag { name: "Military".into(), rank: Some(80), spoiler: Some(false) }],
            studios: vec![],
            producers: vec![],
            licensors: vec![],
            age_rating: Some("R17+".into()),
            relations: vec![],
            external_links: vec![],
            streaming: vec![],
            recommendations: vec![],
            characters: vec![],
            staff: vec![],
            library: None,
            updated_at: Some(1),
        };
        let v = value(&d);
        assert!(v.get("library").is_none());
        assert_eq!(v["ids"]["anilist"], json!(1));
        assert_eq!(v["ids"]["kitsu"], json!(null));
        assert_eq!(v["trailer"]["url"], json!("https://www.youtube.com/watch?v=abc"));
        assert!(v["trailer"].get("thumbnail").is_none());
        assert_eq!(v["tags"][0]["spoiler"], json!(false));
        // Arrays are always present, even when empty: the client iterates them
        // unconditionally.
        assert_eq!(v["genres"], json!([]));
        assert_eq!(v["relations"], json!([]));
    }

    #[test]
    fn a_detail_response_with_a_library_entry_keeps_it() {
        let entry = LibraryEntry {
            uid: "al:1".into(),
            status: "watching".into(),
            is_favorite: true,
            score: Some(9),
            progress: Some(3),
            episodes: Some(25),
            notes: None,
            updated_at: 42,
        };
        let v = value(&entry);
        assert_eq!(v["status"], json!("watching"));
        assert_eq!(v["is_favorite"], json!(true));
        assert_eq!(v["notes"], json!(null));
    }

    // ------------------------------------------------------ request bodies

    #[test]
    fn a_register_body_accepts_an_optional_email() {
        let b: RegisterBody = serde_json::from_value(json!({
            "username": "user", "password": "password123"
        }))
        .unwrap();
        assert_eq!(b.username, "user");
        assert_eq!(b.email, None);
        assert_eq!(b.password, "password123");
    }

    #[test]
    fn a_register_body_requires_a_username_and_a_password() {
        assert!(serde_json::from_value::<RegisterBody>(json!({ "username": "u" })).is_err());
        assert!(serde_json::from_value::<RegisterBody>(json!({ "password": "p" })).is_err());
    }

    #[test]
    fn a_login_body_is_username_or_email_under_one_field() {
        let b: LoginBody = serde_json::from_value(json!({ "login": "a@b.c", "password": "x" }))
            .unwrap();
        assert_eq!(b.login, "a@b.c");
    }

    // ---------------------------------------------------------- auth output

    #[test]
    fn an_auth_response_carries_the_token_and_the_public_user() {
        let r = AuthResponse {
            token: "tok".into(),
            expires_at: 123,
            user: PublicUser {
                id: 1,
                username: "user".into(),
                email: None,
                created_at: 7,
            },
        };
        let v = value(&r);
        assert_eq!(v["token"], json!("tok"));
        assert_eq!(v["expires_at"], json!(123));
        assert_eq!(v["user"]["username"], json!("user"));
        // `password_hash` is not a field of `PublicUser` at all, which is the
        // guarantee that it cannot leak.
        assert!(v["user"].get("password_hash").is_none());
    }
}

