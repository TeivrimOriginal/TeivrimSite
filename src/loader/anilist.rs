use super::Ctx;
use crate::db;
use crate::error::{log_error, log_info, log_warn};
use crate::sources::anilist::{self, Media};
use crate::error::now_ts;
use rusqlite::{named_params, Connection};
use std::time::Instant;

const SOURCE: &str = "anilist";
/// Guards against a pathological loop if AniList keeps reporting next pages.
const MAX_PAGES: u32 = 4_000;

pub async fn run(ctx: Ctx) -> Result<(), String> {
    let per_page = ctx.cfg.sync_page_size;
    let t0 = Instant::now();

    for sort in anilist::SORTS {
        if ctx.abort_requested() {
            log_info("[anilist] прервано по запросу");
            return Ok(());
        }

        let task = format!("sort:{}", sort);
        let cp = with_conn(&ctx, |c| Ok(db::get_checkpoint(c, SOURCE, &task)))?;

        if cp.finished {
            log_info(&format!("[anilist][{}] уже синхронизирован, пропускаю", sort));
            continue;
        }

        let start_page = (cp.last_page + 1).max(1) as u32;
        if start_page > 1 {
            log_info(&format!("[anilist][{}] продолжаю со страницы {}", sort, start_page));
        }

        let mut saved: i64 = 0;
        let mut page = start_page;
        let mut consecutive_errors = 0u32;
        let mut last_reported = Instant::now();

        while page <= MAX_PAGES {
            let (media, has_next) = match anilist::fetch_page(&ctx.sources.anilist, page, per_page, sort).await {
                Ok(f) => (f.media, f.has_next),
                Err(e) => {
                    consecutive_errors += 1;
                    // The upstream client already retried with backoff, so a
                    // failure here means the source really is unavailable.
                    log_error(&format!("[anilist][{}] стр. {}: {}", sort, page, e));
                    let _ = with_conn(&ctx, |c| { db::mark_error(c, SOURCE, &task, &e); Ok(()) });
                    if consecutive_errors >= 3 {
                        log_error(&format!(
                            "[anilist][{}] три ошибки подряд, перехожу к следующей сортировке",
                            sort
                        ));
                        break;
                    }
                    page += 1;
                    continue;
                }
            };
            consecutive_errors = 0;

            if media.is_empty() {
                with_conn(&ctx, |c| db::save_checkpoint(c, SOURCE, &task, page as i64, cp.total_saved + saved, true))?;
                break;
            }

            let mut ok = 0i64;
            let mut touched: Vec<String> = Vec::with_capacity(media.len());
            for m in &media {
                match with_conn(&ctx, |c| upsert(c, m)) {
                    Ok(()) => {
                        ok += 1;
                        touched.push(format!("al:{}", m.id));
                    }
                    Err(e) => log_warn(&format!("[anilist] id={}: {}", m.id, e)),
                }
            }
            saved += ok;

            with_conn(&ctx, |c| {
                db::save_checkpoint(c, SOURCE, &task, page as i64, cp.total_saved + saved, !has_next)
            })?;

            // Link genres for this page straight away so the filter is usable
            // while the import is still running.
            if !touched.is_empty() {
                let _ = with_conn(&ctx, |c| Ok(super::genres::match_uids(c, &touched)));
            }

            // One line every 10 pages, or at most every 15 seconds, so a long
            // import does not fill the log with 1800 identical rows.
            if !has_next || page % 10 == 0 || last_reported.elapsed().as_secs() >= 15 {
                last_reported = Instant::now();
                let total = with_conn(&ctx, |c| {
                    c.query_row("SELECT COUNT(*) FROM anime", [], |r| r.get::<_, i64>(0))
                })
                .unwrap_or(0);
                log_info(&format!(
                    "[anilist][{}] стр. {}: +{} (задача: {}), в БД: {}",
                    sort, page, ok, saved, total
                ));
            }

            if !has_next {
                break;
            }
            page += 1;
        }
    }

    log_info(&format!(
        "[anilist] готово за {}",
        super::human_secs(t0.elapsed().as_secs())
    ));
    Ok(())
}

fn with_conn<T>(ctx: &Ctx, f: impl FnOnce(&Connection) -> Result<T, rusqlite::Error>) -> Result<T, String> {
    let c = ctx.db.conn().map_err(|e| e.to_string())?;
    f(&c).map_err(|e| {
        log_error(&format!("db: {}", e));
        e.to_string()
    })
}

fn upsert(conn: &Connection, m: &Media) -> Result<(), rusqlite::Error> {
    let t = &m.title;
    let uid = format!("al:{}", m.id);

    let (cover_xl, cover_l, cover_m, cover_color) = match &m.cover_image {
        Some(c) => (c.extra_large.clone(), c.large.clone(), c.medium.clone(), c.color.clone()),
        None => (None, None, None, None),
    };
    let (tr_id, tr_site, tr_thumb) = match &m.trailer {
        Some(t) => (t.id.clone(), t.site.clone(), t.thumbnail.clone()),
        None => (None, None, None),
    };

    let sd = m.start_date.as_ref();
    let ed = m.end_date.as_ref();
    let key = t.romaji.as_deref().map(super::title_key);

    // Any name we were not already storing goes into alt_titles so search can
    // find the row by all of them.
    let mut alt: Vec<String> = Vec::new();
    for v in [t.romaji.as_deref(), t.english.as_deref(), t.native.as_deref(), t.user_preferred.as_deref()]
        .into_iter()
        .flatten()
    {
        if !v.trim().is_empty() && !alt.iter().any(|x| x == v) {
            alt.push(v.to_string());
        }
    }
    if let Some(syn) = &m.synonyms {
        for s in syn {
            if !s.trim().is_empty() && !alt.iter().any(|x| x == s) {
                alt.push(s.clone());
            }
        }
    }
    let alt_json = if alt.is_empty() { None } else { serde_json::to_string(&alt).ok() };

    let json_of = |v: &Option<serde_json::Value>| v.as_ref().and_then(|x| serde_json::to_string(x).ok());

    let studios = m.studios.as_ref().and_then(|s| s.nodes.as_ref()).map(|n| {
        serde_json::json!(n
            .iter()
            .map(|s| serde_json::json!({ "name": s.name, "isAnimationStudio": s.is_animation_studio }))
            .collect::<Vec<_>>())
    });

    let relations = m.relations.as_ref().and_then(|r| r.edges.as_ref()).map(|edges| {
        serde_json::json!(edges
            .iter()
            .filter_map(|e| e.node.as_ref().map(|n| serde_json::json!({
                "relationType": e.relation_type,
                "id": n.id,
                "title": n.title,
                "format": n.format,
                "status": n.status,
                "cover": n.cover_image.as_ref().and_then(|c| c.large.clone()),
            })))
            .collect::<Vec<_>>())
    });

    let external_links = m.external_links.as_ref().map(|links| {
        serde_json::json!(links
            .iter()
            .map(|l| serde_json::json!({ "url": l.url, "site": l.site, "type": l.link_type }))
            .collect::<Vec<_>>())
    });

    let streaming = m.streaming_episodes.as_ref().map(|eps| {
        serde_json::json!(eps
            .iter()
            .map(|e| serde_json::json!({ "title": e.title, "thumbnail": e.thumbnail, "url": e.url, "site": e.site }))
            .collect::<Vec<_>>())
    });

    let recs = m.recommendations.as_ref().and_then(|r| r.nodes.as_ref()).map(|nodes| {
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
    });

    let b = |v: Option<bool>| v.map(|x| if x { 1 } else { 0 });

    // Named parameters throughout. The positional form this replaces had 52
    // columns against 49 placeholders after a column was added, and the
    // mismatch only showed up as a runtime error on every single row.
    conn.execute(
        r#"
        INSERT INTO anime (
            uid, anilist_id, mal_id,
            title_romaji, title_english, title_native, title_key, alt_titles,
            format, status, description, duration, episodes, chapters, volumes,
            country_of_origin, is_adult, is_licensed,
            season, season_year,
            start_date, end_date, start_year, start_month, start_day,
            end_year, end_month, end_day,
            score, score_source, mean_score, popularity, favourites, trending,
            cover_small, cover_medium, cover_large, cover_color, banner,
            trailer_id, trailer_site, trailer_thumbnail,
            genres_json, tags_json, studios_json, relations_json,
            external_links_json, streaming_json, recommendations_json,
            created_at, updated_at, anilist_synced_at
        ) VALUES (
            :uid, :anilist_id, :mal_id,
            :title_romaji, :title_english, :title_native, :title_key, :alt_titles,
            :format, :status, :description, :duration, :episodes, :chapters, :volumes,
            :country_of_origin, :is_adult, :is_licensed,
            :season, :season_year,
            :start_date, :end_date, :start_year, :start_month, :start_day,
            :end_year, :end_month, :end_day,
            :score, 'anilist', :mean_score, :popularity, :favourites, :trending,
            -- AniList calls these extraLarge / large / medium; the schema
            -- stores large / medium / small so a client picks by density
            -- without knowing the source vocabulary.
            :cover_small, :cover_medium, :cover_large, :cover_color, :banner,
            :trailer_id, :trailer_site, :trailer_thumbnail,
            :genres_json, :tags_json, :studios_json, :relations_json,
            :external_links_json, :streaming_json, :recommendations_json,
            :now, :now, :now
        )
        ON CONFLICT(uid) DO UPDATE SET
            anilist_id   = COALESCE(excluded.anilist_id, anime.anilist_id),
            mal_id       = COALESCE(excluded.mal_id, anime.mal_id),
            title_romaji = COALESCE(excluded.title_romaji, anime.title_romaji),
            title_english= COALESCE(excluded.title_english, anime.title_english),
            title_native = COALESCE(excluded.title_native, anime.title_native),
            title_key    = COALESCE(excluded.title_key, anime.title_key),
            alt_titles   = COALESCE(excluded.alt_titles, anime.alt_titles),
            format       = COALESCE(excluded.format, anime.format),
            status       = COALESCE(excluded.status, anime.status),
            description  = COALESCE(excluded.description, anime.description),
            duration     = COALESCE(excluded.duration, anime.duration),
            episodes     = COALESCE(excluded.episodes, anime.episodes),
            chapters     = COALESCE(excluded.chapters, anime.chapters),
            volumes      = COALESCE(excluded.volumes, anime.volumes),
            country_of_origin = COALESCE(excluded.country_of_origin, anime.country_of_origin),
            is_adult     = COALESCE(excluded.is_adult, anime.is_adult),
            is_licensed  = COALESCE(excluded.is_licensed, anime.is_licensed),
            season       = COALESCE(excluded.season, anime.season),
            season_year  = COALESCE(excluded.season_year, anime.season_year),
            start_date   = COALESCE(excluded.start_date, anime.start_date),
            end_date     = COALESCE(excluded.end_date, anime.end_date),
            start_year   = COALESCE(excluded.start_year, anime.start_year),
            start_month  = COALESCE(excluded.start_month, anime.start_month),
            start_day    = COALESCE(excluded.start_day, anime.start_day),
            end_year     = COALESCE(excluded.end_year, anime.end_year),
            end_month    = COALESCE(excluded.end_month, anime.end_month),
            end_day      = COALESCE(excluded.end_day, anime.end_day),
            score        = COALESCE(excluded.score, anime.score),
            score_source = COALESCE(excluded.score_source, anime.score_source),
            mean_score   = COALESCE(excluded.mean_score, anime.mean_score),
            popularity   = COALESCE(excluded.popularity, anime.popularity),
            favourites   = COALESCE(excluded.favourites, anime.favourites),
            trending     = COALESCE(excluded.trending, anime.trending),
            cover_small  = COALESCE(excluded.cover_small, anime.cover_small),
            cover_medium = COALESCE(excluded.cover_medium, anime.cover_medium),
            cover_large  = COALESCE(excluded.cover_large, anime.cover_large),
            cover_color  = COALESCE(excluded.cover_color, anime.cover_color),
            banner       = COALESCE(excluded.banner, anime.banner),
            trailer_id   = COALESCE(excluded.trailer_id, anime.trailer_id),
            trailer_site = COALESCE(excluded.trailer_site, anime.trailer_site),
            trailer_thumbnail = COALESCE(excluded.trailer_thumbnail, anime.trailer_thumbnail),
            genres_json  = COALESCE(excluded.genres_json, anime.genres_json),
            tags_json    = COALESCE(excluded.tags_json, anime.tags_json),
            studios_json = COALESCE(excluded.studios_json, anime.studios_json),
            relations_json = COALESCE(excluded.relations_json, anime.relations_json),
            external_links_json = COALESCE(excluded.external_links_json, anime.external_links_json),
            streaming_json = COALESCE(excluded.streaming_json, anime.streaming_json),
            recommendations_json = COALESCE(excluded.recommendations_json, anime.recommendations_json),
            updated_at   = excluded.updated_at,
            anilist_synced_at = excluded.anilist_synced_at
        "#,
        named_params! {
            ":uid": uid,
            ":anilist_id": m.id,
            ":mal_id": m.id_mal,
            ":title_romaji": t.romaji,
            ":title_english": t.english,
            ":title_native": t.native,
            ":title_key": key,
            ":alt_titles": alt_json,
            ":format": m.format,
            ":status": m.status,
            ":description": m.description.as_deref().map(super::strip_html),
            ":duration": m.duration,
            ":episodes": m.episodes,
            ":chapters": m.chapters,
            ":volumes": m.volumes,
            ":country_of_origin": m.country_of_origin,
            ":is_adult": b(m.is_adult),
            ":is_licensed": b(m.is_licensed),
            ":season": m.season,
            ":season_year": m.season_year,
            ":start_date": sd.and_then(|d| d.to_iso()),
            ":end_date": ed.and_then(|d| d.to_iso()),
            ":start_year": sd.and_then(|d| d.year),
            ":start_month": sd.and_then(|d| d.month),
            ":start_day": sd.and_then(|d| d.day),
            ":end_year": ed.and_then(|d| d.year),
            ":end_month": ed.and_then(|d| d.month),
            ":end_day": ed.and_then(|d| d.day),
            ":score": m.average_score,
            ":mean_score": m.mean_score,
            ":popularity": m.popularity,
            ":favourites": m.favourites,
            ":trending": m.trending,
            ":cover_small": cover_m,
            ":cover_medium": cover_l,
            ":cover_large": cover_xl,
            ":cover_color": cover_color,
            ":banner": m.banner_image,
            ":trailer_id": tr_id,
            ":trailer_site": tr_site,
            ":trailer_thumbnail": tr_thumb,
            ":genres_json": m.genres.as_ref().map(|g| serde_json::to_string(g).unwrap_or_default()),
            ":tags_json": json_of(&m.tags.as_ref().map(|t| serde_json::to_value(t).unwrap_or_default())),
            ":studios_json": json_of(&studios),
            ":relations_json": json_of(&relations),
            ":external_links_json": json_of(&external_links),
            ":streaming_json": json_of(&streaming),
            ":recommendations_json": json_of(&recs),
            ":now": now_ts()
        },
    )?;
    Ok(())
}
