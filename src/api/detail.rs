use crate::error::{ApiError, ApiResult};
use crate::models::*;
use rusqlite::{Connection, Row};
use serde_json::Value;

/// Loads the full detail for one `uid`.
///
/// v1 did `SELECT *` and handed the client whatever the columns were named
/// (`genres_json`, `tags_json`, `relations_json`, …). The frontend asked for
/// `genres`, `tags`, `relations`, so every one of those sections rendered as
/// empty, and `a.id` was checked where the column is `anilist_id`, so the id
/// row was missing too. This reads explicit columns into explicit fields.
pub fn load_detail(
    conn: &Connection,
    uid: &str,
    _fts: bool,
    user_id: Option<i64>,
) -> ApiResult<AnimeDetail> {
    let row = conn
        .query_row(&format!("SELECT {} FROM anime a WHERE a.uid = ?1", DETAIL_COLUMNS), [uid], |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?, // anilist_id
                r.get::<_, Option<i64>>(1)?, // kitsu_id
                r.get::<_, Option<i64>>(2)?, // shikimori_id
                r.get::<_, Option<i64>>(3)?, // mal_id
                r.get::<_, Option<String>>(4)?,  // title_romaji
                r.get::<_, Option<String>>(5)?,  // title_english
                r.get::<_, Option<String>>(6)?,  // title_native
                r.get::<_, Option<String>>(7)?,  // title_russian
                r.get::<_, Option<String>>(8)?,  // alt_titles
                r.get::<_, Option<String>>(9)?,  // format
                r.get::<_, Option<String>>(10)?, // status
                r.get::<_, Option<String>>(11)?, // description
                r.get::<_, Option<String>>(12)?, // description_ru
                r.get::<_, Option<i64>>(13)?,    // duration
                r.get::<_, Option<i64>>(14)?,    // episodes
                r.get::<_, Option<i64>>(15)?,    // chapters
                r.get::<_, Option<i64>>(16)?,    // volumes
                r.get::<_, Option<String>>(17)?, // country
                r.get::<_, Option<i64>>(18)?,    // is_adult
                r.get::<_, Option<i64>>(19)?,    // is_licensed
                r.get::<_, Option<String>>(20)?, // season
                r.get::<_, Option<i64>>(21)?,    // season_year
                r.get::<_, Option<String>>(22)?, // start_date
                r.get::<_, Option<String>>(23)?, // end_date
                r.get::<_, Option<i64>>(24)?,    // score
                r.get::<_, Option<String>>(25)?, // score_source
                r.get::<_, Option<i64>>(26)?,    // mean_score
                r.get::<_, Option<i64>>(27)?,    // popularity
                r.get::<_, Option<i64>>(28)?,    // favourites
                r.get::<_, Option<i64>>(29)?,    // trending
                r.get::<_, Option<i64>>(30)?,    // rating_count
                r.get::<_, Option<String>>(31)?, // cover_small
                r.get::<_, Option<String>>(32)?, // cover_medium
                r.get::<_, Option<String>>(33)?, // cover_large
                r.get::<_, Option<String>>(34)?, // cover_color
                r.get::<_, Option<String>>(35)?, // banner
                r.get::<_, Option<String>>(36)?, // trailer_id
                r.get::<_, Option<String>>(37)?, // trailer_site
                r.get::<_, Option<String>>(38)?, // trailer_thumbnail
                r.get::<_, Option<String>>(39)?, // genres_json
                r.get::<_, Option<String>>(40)?, // tags_json
                r.get::<_, Option<String>>(41)?, // studios_json
                r.get::<_, Option<String>>(42)?, // producers_json
                r.get::<_, Option<String>>(43)?, // licensors_json
                r.get::<_, Option<String>>(44)?, // classifications_json
                r.get::<_, Option<String>>(45)?, // relations_json
                r.get::<_, Option<String>>(46)?, // external_links_json
                r.get::<_, Option<String>>(47)?, // streaming_json
                r.get::<_, Option<String>>(48)?, // recommendations_json
                r.get::<_, Option<String>>(49)?, // characters_json
                r.get::<_, Option<String>>(50)?, // staff_json
                r.get::<_, Option<i64>>(51)?,    // updated_at
            ))
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => ApiError::NotFound("Аниме не найдено".into()),
            other => ApiError::from(other),
        })?;

    let mut detail = AnimeDetail {
        uid: uid.to_string(),
        ids: SourceIds {
            anilist: row.0,
            kitsu: row.1,
            shikimori: row.2,
            mal: row.3,
        },
        title_romaji: row.4.clone(),
        title_english: row.5.clone(),
        title_native: row.6.clone(),
        title_russian: row.7.clone(),
        // No machine translation happens any more: Shikimori covers Russian
        // titles, and scraping Google got the server IP banned. Every other
        // name the sources know about lives in `alt_titles`.
        synonyms: {
            let mut v: Vec<String> = parse_string_array(&row.8);
            for extra in [
                row.4.as_deref(),
                row.5.as_deref(),
                row.6.as_deref(),
                row.7.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                let t = extra.trim();
                if !t.is_empty() && !v.iter().any(|x| x == t) {
                    v.push(t.to_string());
                }
            }
            v
        },
        format: row.9.clone(),
        status: row.10.clone(),
        description: row.11.clone(),
        description_ru: row.12.clone(),
        duration: row.13,
        episodes: row.14,
        chapters: row.15,
        volumes: row.16,
        country: row.17.clone(),
        is_adult: row.18.unwrap_or(0) == 1,
        is_licensed: row.19.map(|v| v == 1),
        season: row.20.clone(),
        season_year: row.21,
        start_date: row.22.clone(),
        end_date: row.23.clone(),
        score: row.24,
        score_source: row.25.clone(),
        mean_score: row.26,
        popularity: row.27,
        favourites: row.28,
        trending: row.29,
        rating_count: row.30,
        cover_small: row.31.clone(),
        cover_medium: row.32.clone(),
        cover_large: row.33.clone(),
        cover_color: row.34.clone(),
        banner: row.35.clone(),
        trailer: match (row.36.as_deref(), row.37.as_deref()) {
            (Some(id), Some(site)) if !id.is_empty() => Some(Trailer {
                site: site.to_string(),
                id: id.to_string(),
                url: watch_url(site, id),
                thumbnail: row.38.clone(),
            }),
            _ => None,
        },
        genres: crate::api::catalog::parse_genres(&row.39),
        tags: parse_tags(&row.40),
        studios: parse_named(&row.41),
        producers: parse_named(&row.42),
        licensors: parse_named(&row.43),
        age_rating: parse_age_rating(&row.44),
        relations: parse_relations(&row.45),
        external_links: parse_external_links(&row.46),
        streaming: parse_streaming(&row.47),
        recommendations: parse_recommendations(&row.48),
        characters: parse_people(&row.49, true),
        staff: parse_people(&row.50, false),
        library: None,
        updated_at: row.51,
    };

    // Prefer the curated genre list from `genres` (which carries Russian
    // names), falling back to the raw AniList array when the matcher has not
    // run yet.
    if let Some(from_dict) = genres_from_dict(conn, uid)? {
        if !from_dict.is_empty() {
            detail.genres = from_dict;
        }
    }

    if let Some(uid_user) = user_id {
        detail.library = load_library_entry(conn, uid_user, uid)?;
    }

    Ok(detail)
}

const DETAIL_COLUMNS: &str = "a.anilist_id, a.kitsu_id, a.shikimori_id, a.mal_id, \
    a.title_romaji, a.title_english, a.title_native, a.title_russian, a.alt_titles, \
    a.format, a.status, a.description, a.description_ru, \
    a.duration, a.episodes, a.chapters, a.volumes, a.country_of_origin, \
    a.is_adult, a.is_licensed, \
    a.season, a.season_year, a.start_date, a.end_date, \
    a.score, a.score_source, a.mean_score, a.popularity, a.favourites, a.trending, a.rating_count, \
    a.cover_small, a.cover_medium, a.cover_large, a.cover_color, a.banner, \
    a.trailer_id, a.trailer_site, a.trailer_thumbnail, \
    a.genres_json, a.tags_json, a.studios_json, a.producers_json, a.licensors_json, \
    a.classifications_json, a.relations_json, a.external_links_json, a.streaming_json, \
    a.recommendations_json, a.characters_json, a.staff_json, \
    a.updated_at";

fn watch_url(site: &str, id: &str) -> String {
    match site.to_ascii_lowercase().as_str() {
        "youtube" => format!("https://www.youtube.com/watch?v={}", id),
        "dailymotion" => format!("https://www.dailymotion.com/video/{}", id),
        "twitch" => format!("https://www.twitch.tv/videos/{}", id),
        other => format!("https://{}/", other),
    }
}

fn genres_from_dict(conn: &Connection, uid: &str) -> ApiResult<Option<Vec<Genre>>> {
    let mut stmt = conn.prepare_cached(
        "SELECT g.id, g.slug, g.name_en, g.name_ru, g.category
         FROM anime_genres ag JOIN genres g ON g.id = ag.genre_id
         WHERE ag.uid = ?1 AND (g.category = 'genre' OR g.category IS NULL)
         ORDER BY g.name_en",
    )?;
    let rows = stmt.query_map([uid], |r: &Row<'_>| {
        Ok(Genre {
            id: r.get(0)?,
            slug: r.get(1)?,
            name: r.get(2)?,
            name_ru: r.get(3)?,
            category: r.get(4)?,
            count: None,
        })
    })?;
    let mut out = Vec::new();
    for g in rows.flatten() {
        out.push(g);
    }
    Ok(if out.is_empty() { None } else { Some(out) })
}

pub fn load_library_entry(conn: &Connection, user_id: i64, uid: &str) -> ApiResult<Option<LibraryEntry>> {
    let found = conn
        .query_row(
            "SELECT status, is_favorite, score, progress, episodes, notes, updated_at
             FROM favorites WHERE user_id = ?1 AND uid = ?2",
            rusqlite::params![user_id, uid],
            |r| {
                Ok(LibraryEntry {
                    uid: uid.to_string(),
                    status: r.get(0)?,
                    is_favorite: r.get::<_, i64>(1)? == 1,
                    score: r.get(2)?,
                    progress: r.get(3)?,
                    episodes: r.get(4)?,
                    notes: r.get(5)?,
                    updated_at: r.get(6)?,
                })
            },
        )
        .ok();
    Ok(found)
}

fn parse_string_array(json: &Option<String>) -> Vec<String> {
    json.as_ref()
        .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
        .unwrap_or_default()
}

fn parse_tags(json: &Option<String>) -> Vec<Tag> {
    let Some(s) = json else { return Vec::new() };
    serde_json::from_str::<Vec<Value>>(s)
        .unwrap_or_default()
        .iter()
        .filter_map(|v| {
            v.get("name")
                .and_then(|n| n.as_str())
                .map(|name| Tag {
                    name: name.to_string(),
                    rank: v.get("rank").and_then(|r| r.as_i64()),
                    spoiler: v.get("isMediaSpoiler").and_then(|s| s.as_bool()),
                })
        })
        .collect()
}

fn parse_named(json: &Option<String>) -> Vec<NamedRef> {
    let Some(s) = json else { return Vec::new() };
    serde_json::from_str::<Vec<Value>>(s)
        .unwrap_or_default()
        .iter()
        .filter_map(|v| {
            let name = v
                .get("name")
                .and_then(|n| n.as_str())
                .or_else(|| v.as_str())?;
            Some(NamedRef {
                name: name.to_string(),
                name_ru: v.get("name_ru").and_then(|n| n.as_str()).map(|s| s.to_string()),
                is_main: v.get("isAnimationStudio").and_then(|b| b.as_bool()),
            })
        })
        .collect()
}

fn parse_age_rating(json: &Option<String>) -> Option<String> {
    let s = json.as_ref()?;
    let v: Value = serde_json::from_str(s).ok()?;
    v.get("ageRatingGuide")
        .or_else(|| v.get("ageRating"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
}

fn title_from_value(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    for key in ["russian", "romaji", "english", "native", "userPreferred"] {
        if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    "Без названия".to_string()
}

fn parse_relations(json: &Option<String>) -> Vec<Relation> {
    let Some(s) = json else { return Vec::new() };
    serde_json::from_str::<Vec<Value>>(s)
        .unwrap_or_default()
        .iter()
        .filter_map(|v| {
            let id = v.get("id").and_then(|x| x.as_i64())?;
            Some(Relation {
                relation: v
                    .get("relationType")
                    .and_then(|x| x.as_str())
                    .unwrap_or("RELATED")
                    .replace('_', " ")
                    .to_lowercase(),
                uid: format!("al:{}", id),
                title: v.get("title").map(title_from_value).unwrap_or_default(),
                format: v.get("format").and_then(|x| x.as_str()).map(|s| s.to_string()),
                status: v.get("status").and_then(|x| x.as_str()).map(|s| s.to_string()),
                cover: v
                    .get("cover")
                    .or_else(|| v.get("coverImage"))
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string()),
            })
        })
        .collect()
}

fn parse_external_links(json: &Option<String>) -> Vec<ExternalLink> {
    let Some(s) = json else { return Vec::new() };
    serde_json::from_str::<Vec<Value>>(s)
        .unwrap_or_default()
        .iter()
        .filter_map(|v| {
            // Only http(s) survives: these URLs come from an upstream API and
            // are rendered as href on the client.
            let url = v.get("url").and_then(|x| x.as_str())?;
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                return None;
            }
            Some(ExternalLink {
                site: v
                    .get("site")
                    .and_then(|x| x.as_str())
                    .unwrap_or("link")
                    .to_string(),
                url: url.to_string(),
                kind: v.get("type").and_then(|x| x.as_str()).map(|s| s.to_string()),
            })
        })
        .collect()
}

fn parse_streaming(json: &Option<String>) -> Vec<StreamingLink> {
    let Some(s) = json else { return Vec::new() };
    serde_json::from_str::<Vec<Value>>(s)
        .unwrap_or_default()
        .iter()
        .filter_map(|v| {
            let url = v.get("url").and_then(|x| x.as_str())?;
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                return None;
            }
            Some(StreamingLink {
                site: v.get("site").and_then(|x| x.as_str()).unwrap_or("watch").to_string(),
                url: url.to_string(),
                title: v.get("title").and_then(|x| x.as_str()).map(|s| s.to_string()),
                thumbnail: v.get("thumbnail").and_then(|x| x.as_str()).map(|s| s.to_string()),
            })
        })
        .collect()
}

fn parse_recommendations(json: &Option<String>) -> Vec<Recommendation> {
    let Some(s) = json else { return Vec::new() };
    serde_json::from_str::<Vec<Value>>(s)
        .unwrap_or_default()
        .iter()
        .filter_map(|v| {
            let id = v.get("id").and_then(|x| x.as_i64())?;
            Some(Recommendation {
                uid: format!("al:{}", id),
                title: v.get("title").map(title_from_value).unwrap_or_default(),
                rating: v.get("rating").and_then(|x| x.as_i64()),
                format: v.get("format").and_then(|x| x.as_str()).map(|s| s.to_string()),
                cover: v.get("cover").and_then(|x| x.as_str()).map(|s| s.to_string()),
            })
        })
        .collect()
}

/// Characters and staff share a shape, so one parser covers both. The
/// difference is which extra field each carries.
fn parse_people(json: &Option<String>, with_voice: bool) -> Vec<Person> {
    let Some(s) = json else { return Vec::new() };
    let arr: Vec<Value> = serde_json::from_str(s).unwrap_or_default();
    let mut out = Vec::new();
    for v in arr {
        if with_voice {
            let name = v
                .get("character")
                .and_then(|c| c.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            out.push(Person {
                name: name.to_string(),
                image: v
                    .get("character")
                    .and_then(|c| c.get("image"))
                    .and_then(|i| i.get("large"))
                    .and_then(|l| l.as_str())
                    .map(|s| s.to_string()),
                role: v.get("role").and_then(|r| r.as_str()).map(|s| s.to_string()),
                voice_actor: v
                    .get("voiceActors")
                    .and_then(|va| va.as_array())
                    .and_then(|list| {
                        // Prefer the Japanese VA, fall back to whatever is first.
                        list.iter()
                            .find(|x| x.get("language").and_then(|l| l.as_str()) == Some("Japanese"))
                            .or_else(|| list.first())
                    })
                    .and_then(|x| x.get("name"))
                    .and_then(|n| n.as_str())
                    .map(|s| s.to_string()),
                positions: None,
            });
        } else {
            let name = v
                .get("person")
                .and_then(|c| c.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or_default();
            if name.is_empty() {
                continue;
            }
            out.push(Person {
                name: name.to_string(),
                image: v
                    .get("person")
                    .and_then(|c| c.get("image"))
                    .and_then(|i| i.get("large"))
                    .and_then(|l| l.as_str())
                    .map(|s| s.to_string()),
                role: None,
                voice_actor: None,
                positions: v.get("positions").and_then(|p| p.as_array()).map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                }),
            });
        }
    }
    out
}

/// Whole-row dump for debugging, used by `?raw=1` on the detail endpoint.
pub fn raw_row(conn: &Connection, uid: &str) -> ApiResult<Option<Value>> {
    let mut stmt = conn.prepare("SELECT * FROM anime WHERE uid = ?1")?;
    let mut rows = stmt.query([uid])?;
    let Some(row) = rows.next()? else { return Ok(None) };
    let mut obj = serde_json::Map::new();
    for i in 0..row.as_ref().column_count() {
        let name = row.as_ref().column_name(i).unwrap_or("?").to_string();
        let v: Value = match row.get_ref(i) {
            Ok(rusqlite::types::ValueRef::Null) => Value::Null,
            Ok(rusqlite::types::ValueRef::Integer(n)) => Value::Number(n.into()),
            Ok(rusqlite::types::ValueRef::Real(f)) => {
                serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null)
            }
            Ok(rusqlite::types::ValueRef::Text(t)) => {
                let s = String::from_utf8_lossy(t).to_string();
                if name.ends_with("_json") {
                    serde_json::from_str(&s).unwrap_or(Value::String(s))
                } else {
                    Value::String(s)
                }
            }
            _ => Value::Null,
        };
        obj.insert(name, v);
    }
    Ok(Some(Value::Object(obj)))
}

/// Values used by the filter UI. Cached by the caller.
pub fn distinct_values(conn: &Connection, column: &str) -> ApiResult<Vec<String>> {
    // `column` is never user input — every call site passes a literal.
    let sql = format!(
        "SELECT DISTINCT {0} AS v FROM anime WHERE {0} IS NOT NULL AND {0} <> '' ORDER BY v",
        column
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    Ok(rows.flatten().collect())
}

pub fn year_bounds(conn: &Connection) -> ApiResult<(Option<i64>, Option<i64>)> {
    conn.query_row(
        "SELECT MIN(start_year), MAX(start_year) FROM anime WHERE start_year IS NOT NULL",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .map_err(ApiError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::testing::conn;
    use rusqlite::params;
    use serde_json::json;

    fn some(s: &str) -> Option<String> {
        Some(s.to_string())
    }

    // ------------------------------------------------------------- trailers

    #[test]
    fn a_trailer_url_is_built_for_the_known_sites() {
        // The stored column is an id; the client needs a link, and each site
        // spells its watch URL differently.
        assert_eq!(watch_url("youtube", "abc"), "https://www.youtube.com/watch?v=abc");
        assert_eq!(watch_url("YouTube", "abc"), "https://www.youtube.com/watch?v=abc");
        assert_eq!(watch_url("dailymotion", "x1"), "https://www.dailymotion.com/video/x1");
        assert_eq!(watch_url("twitch", "v1"), "https://www.twitch.tv/videos/v1");
    }

    #[test]
    fn an_unknown_trailer_site_does_not_invent_a_link() {
        // Better a bare site URL than a broken path into someone else's API.
        assert_eq!(watch_url("weird-host", "abc"), "https://weird-host/");
    }

    // -------------------------------------------------------------- columns

    #[test]
    fn parse_string_array_is_total() {
        assert_eq!(parse_string_array(&some(r#"["a","b"]"#)), vec!["a".to_string(), "b".to_string()]);
        assert!(parse_string_array(&some("not json")).is_empty());
        assert!(parse_string_array(&None).is_empty());
    }

    #[test]
    fn parse_tags_keeps_the_rank_and_the_spoiler_flag() {
        let v = parse_tags(&some(
            r#"[{"name":"Military","rank":80,"isMediaSpoiler":true},{"rank":1},{"name":"X"}]"#,
        ));
        assert_eq!(v.len(), 2, "запись без name пропускается");
        assert_eq!(v[0].name, "Military");
        assert_eq!(v[0].rank, Some(80));
        assert_eq!(v[0].spoiler, Some(true));
        assert_eq!(v[1].name, "X");
        assert_eq!(v[1].rank, None);
    }

    #[test]
    fn parse_tags_survives_a_broken_blob() {
        // A truncated blob from a crashed import must not blank the section.
        assert!(parse_tags(&some("{broken")).is_empty());
        assert!(parse_tags(&None).is_empty());
    }

    #[test]
    fn parse_named_accepts_objects_and_bare_strings() {
        // Studios arrive as objects from two sources and as plain strings from
        // a third; both shapes are in the wild.
        let v = parse_named(&some(r#"[{"name":"Wit Studio","name_ru":"Wit","isAnimationStudio":true},"MAPPA"]"#));
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "Wit Studio");
        assert_eq!(v[0].name_ru.as_deref(), Some("Wit"));
        assert_eq!(v[0].is_main, Some(true));
        assert_eq!(v[1].name, "MAPPA");
        assert_eq!(v[1].name_ru, None);
    }

    #[test]
    fn parse_age_rating_prefers_the_guide_and_falls_back() {
        // The guide is human readable; the raw code is a fallback for records
        // that only carry it.
        assert_eq!(
            parse_age_rating(&some(r#"{"ageRating":"R17+","ageRatingGuide":"17+"}"#)).as_deref(),
            Some("17+")
        );
        assert_eq!(
            parse_age_rating(&some(r#"{"ageRating":"R17+"}"#)).as_deref(),
            Some("R17+")
        );
        assert_eq!(parse_age_rating(&some("{}")), None);
        assert_eq!(parse_age_rating(&None), None);
    }

    // ------------------------------------------------------------ relations

    #[test]
    fn relations_lose_their_underscores_and_gain_an_uid() {
        // The client links to /anime/<uid>, and the raw `PREQUEL` reads badly
        // in a filter chip, so it is humanised.
        let v = parse_relations(&some(
            r#"[{"relationType":"PREQUEL","id":11061,"title":{"romaji":"Kaban"},"format":"MOVIE","status":"FINISHED","cover":"c.jpg"}]"#,
        ));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].relation, "prequel");
        assert_eq!(v[0].uid, "al:11061");
        assert_eq!(v[0].title, "Kaban");
        assert_eq!(v[0].format.as_deref(), Some("MOVIE"));
        assert_eq!(v[0].cover.as_deref(), Some("c.jpg"));
    }

    #[test]
    fn a_relation_without_a_type_is_still_rendered() {
        let v = parse_relations(&some(r#"[{"id":1,"title":"X"}]"#));
        assert_eq!(v[0].relation, "related");
    }

    #[test]
    fn a_relation_without_an_id_is_dropped() {
        // There is no uid to link to without the id.
        assert!(parse_relations(&some(r#"[{"relationType":"PREQUEL","title":"X"}]"#)).is_empty());
    }

    #[test]
    fn title_from_value_tries_every_locale_in_order() {
        assert_eq!(title_from_value(&json!("Прямая строка")), "Прямая строка");
        assert_eq!(title_from_value(&json!({ "russian": "RU" })), "RU");
        assert_eq!(title_from_value(&json!({ "romaji": "RJ" })), "RJ");
        assert_eq!(title_from_value(&json!({ "english": "EN" })), "EN");
        assert_eq!(title_from_value(&json!({ "native": "NA" })), "NA");
        assert_eq!(title_from_value(&json!({})), "Без названия");
        // An empty string is skipped, not shown as a blank title.
        assert_eq!(title_from_value(&json!({ "russian": "", "romaji": "RJ" })), "RJ");
    }

    // ------------------------------------------------------------- the rest

    #[test]
    fn only_http_links_reach_the_client() {
        // These URLs are rendered as href. `javascript:` on a page that also
        // holds a token is not a risk worth taking.
        let v = parse_external_links(&some(
            r#"[{"url":"https://a.example/1","site":"MAL","type":"X"},{"url":"http://b.example/2"},{"url":"javascript:alert(1)"},{"url":"data:text/html,x"},{"site":"no-url"}]"#,
        ));
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].site, "MAL");
        assert_eq!(v[0].kind.as_deref(), Some("X"));
        assert_eq!(v[1].site, "link");
    }

    #[test]
    fn only_http_streaming_links_reach_the_client() {
        let v = parse_streaming(&some(
            r#"[{"url":"https://v.example/1","site":"youtube","title":"Ep 1","thumbnail":"t.jpg"},{"url":"javascript:x"}]"#,
        ));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].site, "youtube");
        assert_eq!(v[0].title.as_deref(), Some("Ep 1"));
        assert_eq!(v[0].thumbnail.as_deref(), Some("t.jpg"));
    }

    #[test]
    fn a_streaming_link_without_a_site_gets_a_default() {
        let v = parse_streaming(&some(r#"[{"url":"https://v.example/1"}]"#));
        assert_eq!(v[0].site, "watch");
    }

    #[test]
    fn recommendations_get_their_uid_and_title() {
        let v = parse_recommendations(&some(
            r#"[{"id":127230,"rating":95,"title":{"romaji":"Gingitsune"},"format":"TV_SHORT","cover":"c.jpg"},{"rating":1}]"#,
        ));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].uid, "al:127230");
        assert_eq!(v[0].title, "Gingitsune");
        assert_eq!(v[0].rating, Some(95));
    }

    #[test]
    fn characters_prefer_the_japanese_voice_actor() {
        let v = parse_people(
            &some(
                r#"[{"role":"Main","character":{"name":"Eren","image":{"large":"e.jpg"}},
                     "voiceActors":[{"id":"1","language":"English","name":"VA EN"},
                                    {"id":"2","language":"Japanese","name":"VA JP"}]},
                    {"role":"X","character":{"name":""}}]"#,
            ),
            true,
        );
        assert_eq!(v.len(), 1, "персонаж без имени пропускается");
        assert_eq!(v[0].name, "Eren");
        assert_eq!(v[0].image.as_deref(), Some("e.jpg"));
        assert_eq!(v[0].role.as_deref(), Some("Main"));
        assert_eq!(v[0].voice_actor.as_deref(), Some("VA JP"));
    }

    #[test]
    fn a_character_with_only_a_foreign_voice_actor_still_gets_one() {
        let v = parse_people(
            &some(
                r#"[{"character":{"name":"Eren"},"voiceActors":[{"id":"1","language":"English","name":"VA EN"}]}]"#,
            ),
            true,
        );
        assert_eq!(v[0].voice_actor.as_deref(), Some("VA EN"));
    }

    #[test]
    fn staff_carry_positions_and_never_a_voice_actor() {
        let v = parse_people(
            &some(
                r#"[{"person":{"name":"Sasha","image":{"large":"s.jpg"}},"positions":["Director","Story"]},
                    {"person":{"name":""}}]"#,
            ),
            false,
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "Sasha");
        assert_eq!(v[0].voice_actor, None);
        assert_eq!(v[0].positions, Some(vec!["Director".to_string(), "Story".to_string()]));
    }

    // ------------------------------------------------------- whole-row dump

    #[test]
    fn raw_row_parses_the_json_columns_back_into_objects() {
        // The raw endpoint is a debugging aid; leaving `genres_json` as a
        // string inside JSON is exactly the confusion it exists to remove.
        let c = conn();
        c.execute(
            "INSERT INTO anime (uid, title_romaji, genres_json, start_year, is_adult, created_at)
             VALUES ('al:1', 'X', '[\"Action\"]', 2013, 0, 1)",
            [],
        )
        .unwrap();
        let v = raw_row(&c, "al:1").unwrap().unwrap();
        assert_eq!(v["uid"], json!("al:1"));
        assert_eq!(v["genres_json"], json!(["Action"]));
        assert_eq!(v["start_year"], json!(2013));
        assert_eq!(v["is_adult"], json!(0));
    }

    #[test]
    fn raw_row_of_a_missing_uid_is_none() {
        let c = conn();
        assert!(raw_row(&c, "al:nope").unwrap().is_none());
    }

    #[test]
    fn raw_row_keeps_a_broken_json_column_as_a_string() {
        let c = conn();
        c.execute(
            "INSERT INTO anime (uid, genres_json, created_at) VALUES ('al:1', '{broken', 1)",
            [],
        )
        .unwrap();
        let v = raw_row(&c, "al:1").unwrap().unwrap();
        assert_eq!(v["genres_json"], json!("{broken"));
    }

    // ---------------------------------------------------------- filter lists

    #[test]
    fn distinct_values_skips_nulls_and_blanks_and_sorts() {
        let c = conn();
        for (uid, format) in [("al:1", "TV"), ("al:2", "MOVIE"), ("al:3", ""), ("al:4", "TV")] {
            let f: Option<&str> = if format.is_empty() { None } else { Some(format) };
            c.execute(
                "INSERT INTO anime (uid, format, created_at) VALUES (?1, ?2, 1)",
                params![uid, f],
            )
            .unwrap();
        }
        let v = distinct_values(&c, "format").unwrap();
        assert_eq!(v, vec!["MOVIE".to_string(), "TV".to_string()]);
    }

    #[test]
    fn year_bounds_ignore_yearless_titles() {
        let c = conn();
        c.execute("INSERT INTO anime (uid, start_year, created_at) VALUES ('al:1', 2013, 1)", [])
            .unwrap();
        c.execute("INSERT INTO anime (uid, start_year, created_at) VALUES ('al:2', 2003, 1)", [])
            .unwrap();
        c.execute("INSERT INTO anime (uid, created_at) VALUES ('al:3', 1)", []).unwrap();
        assert_eq!(year_bounds(&c).unwrap(), (Some(2003), Some(2013)));
    }

    #[test]
    fn year_bounds_of_an_empty_catalogue_are_both_null() {
        // The filter sheet has to render "any year" rather than 0..0.
        assert_eq!(year_bounds(&conn()).unwrap(), (None, None));
    }

    // --------------------------------------------------------- the detail row

    fn seeded(conn: &Connection) {
        // A raw string on purpose: the SQL carries JSON, and SQLite has no
        // backslash escapes, so `\"` inside a single-quoted literal would be
        // stored verbatim and the blob would stop parsing as JSON.
        conn.execute(
            r#"INSERT INTO anime (
                uid, anilist_id, kitsu_id, shikimori_id, mal_id,
                title_romaji, title_english, title_russian, alt_titles,
                format, status, description, description_ru,
                duration, episodes, country_of_origin, is_adult, is_licensed,
                season, season_year, start_date, score, score_source, popularity,
                cover_small, cover_medium, cover_large, cover_color, banner,
                trailer_id, trailer_site, trailer_thumbnail,
                genres_json, tags_json, studios_json, classifications_json,
                relations_json, external_links_json, streaming_json,
                recommendations_json, characters_json, staff_json, updated_at
             ) VALUES (
                'al:16498', 16498, 12, 16498, 20,
                'Shingeki no Kyojin', 'Attack on Titan', 'Атака Титанов', '["AoT"]',
                'TV', 'FINISHED', 'desc', 'описание',
                24, 25, 'JP', 0, 1,
                'SPRING', 2013, '2013-04-07', 84, 'anilist', 12345,
                's.jpg', 'm.jpg', 'xl.jpg', '#8f9494', 'b.jpg',
                'abc', 'youtube', 'th.jpg',
                '["Action"]', '[{"name":"Military"}]', '[{"name":"Wit Studio"}]',
                '{"ageRatingGuide":"17+"}',
                '[{"relationType":"PREQUEL","id":11061,"title":{"romaji":"Kaban"}}]',
                '[{"url":"https://myanimelist.net/anime/20","site":"MAL"}]',
                '[{"url":"https://anilist.co/watch/1","site":"anilist"}]',
                '[{"id":127230,"rating":95,"title":{"romaji":"Gingitsune"}}]',
                '[{"role":"Main","character":{"name":"Eren"},"voiceActors":[{"language":"Japanese","name":"VA JP"}]}]',
                '[{"person":{"name":"Sasha"},"positions":["Director"]}]',
                42
             )"#,
            [],
        )
        .unwrap();
    }

    #[test]
    fn load_detail_maps_every_section() {
        let c = conn();
        seeded(&c);
        let d = load_detail(&c, "al:16498", false, None).unwrap();

        assert_eq!(d.uid, "al:16498");
        assert_eq!(d.ids.anilist, Some(16498));
        assert_eq!(d.ids.kitsu, Some(12));
        assert_eq!(d.ids.shikimori, Some(16498));
        assert_eq!(d.ids.mal, Some(20));
        assert_eq!(d.title_russian.as_deref(), Some("Атака Титанов"));
        assert_eq!(d.format.as_deref(), Some("TV"));
        assert!(!d.is_adult);
        assert_eq!(d.is_licensed, Some(true));
        assert_eq!(d.score, Some(84));
        assert_eq!(d.start_date.as_deref(), Some("2013-04-07"));
        assert_eq!(d.cover_large.as_deref(), Some("xl.jpg"));
        assert_eq!(d.cover_color.as_deref(), Some("#8f9494"));
        assert_eq!(d.updated_at, Some(42));

        assert_eq!(d.trailer.as_ref().unwrap().url, "https://www.youtube.com/watch?v=abc");
        assert_eq!(d.genres[0].name, "Action");
        assert_eq!(d.tags[0].name, "Military");
        assert_eq!(d.studios[0].name, "Wit Studio");
        assert_eq!(d.age_rating.as_deref(), Some("17+"));
        assert_eq!(d.relations[0].uid, "al:11061");
        assert_eq!(d.external_links[0].site, "MAL");
        assert_eq!(d.streaming[0].site, "anilist");
        assert_eq!(d.recommendations[0].uid, "al:127230");
        assert_eq!(d.characters[0].name, "Eren");
        assert_eq!(d.characters[0].voice_actor.as_deref(), Some("VA JP"));
        assert_eq!(d.staff[0].name, "Sasha");
    }

    #[test]
    fn synonyms_include_every_stored_title() {
        // Search results link to the detail page, and the client looks the
        // other name up in `synonyms` to highlight it.
        let c = conn();
        seeded(&c);
        let d = load_detail(&c, "al:16498", false, None).unwrap();
        assert!(d.synonyms.contains(&"AoT".to_string()));
        assert!(d.synonyms.contains(&"Shingeki no Kyojin".to_string()));
        assert!(d.synonyms.contains(&"Attack on Titan".to_string()));
        assert!(d.synonyms.contains(&"Атака Титанов".to_string()));
        assert_eq!(
            d.synonyms.iter().filter(|s| *s == "AoT").count(),
            1,
            "дубликат в alt_titles не должен дублироваться в synonyms"
        );
    }

    #[test]
    fn the_genre_dictionary_wins_over_the_stored_array() {
        // The curated table carries ids and Russian names, which the raw array
        // does not; the detail page shows the Russian label.
        let c = conn();
        seeded(&c);
        c.execute(
            "INSERT INTO genres (slug, name_en, name_ru, category) VALUES ('action', 'Action', 'Боевик', 'genre')",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO anime_genres (uid, genre_id, source) SELECT 'al:16498', id, 'anilist' FROM genres WHERE slug = 'action'",
            [],
        )
        .unwrap();
        let d = load_detail(&c, "al:16498", false, None).unwrap();
        assert_eq!(d.genres.len(), 1);
        assert_eq!(d.genres[0].name_ru.as_deref(), Some("Боевик"));
        assert!(d.genres[0].id > 0);
    }

    #[test]
    fn a_row_with_no_dictionary_entries_falls_back_to_the_stored_array() {
        // The genre matcher runs at the end of a sync; until then the detail
        // page must still show something.
        let c = conn();
        seeded(&c);
        let d = load_detail(&c, "al:16498", false, None).unwrap();
        assert_eq!(d.genres[0].name, "Action");
        assert_eq!(d.genres[0].id, 0);
    }

    #[test]
    fn an_unknown_uid_is_a_not_found() {
        let c = conn();
        match load_detail(&c, "al:nope", false, None) {
            Err(ApiError::NotFound(_)) => {}
            Err(other) => panic!("неверная ошибка: {}", other),
            Ok(_) => panic!("ожидался NotFound, а строка вернулась"),
        }
    }

    #[test]
    fn the_library_entry_is_attached_only_for_a_signed_in_user() {
        let c = conn();
        seeded(&c);
        c.execute("INSERT INTO users (username, username_key, password_hash, created_at) VALUES ('u','u','h',1)", [])
            .unwrap();
        c.execute(
            "INSERT INTO favorites (user_id, uid, status, is_favorite, score, created_at, updated_at)
             SELECT id, 'al:16498', 'watching', 1, 9, 1, 42 FROM users",
            [],
        )
        .unwrap();
        let uid: i64 = c.query_row("SELECT id FROM users", [], |r| r.get(0)).unwrap();

        let anonymous = load_detail(&c, "al:16498", false, None).unwrap();
        assert!(anonymous.library.is_none());

        let mine = load_detail(&c, "al:16498", false, Some(uid)).unwrap();
        let lib = mine.library.as_ref().unwrap();
        assert_eq!(lib.status, "watching");
        assert!(lib.is_favorite);
        assert_eq!(lib.score, Some(9));
        assert_eq!(lib.uid, "al:16498");
    }

    #[test]
    fn load_library_entry_of_a_row_not_in_the_list_is_none() {
        let c = conn();
        assert!(load_library_entry(&c, 1, "al:1").unwrap().is_none());
    }
}
