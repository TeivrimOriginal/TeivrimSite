//! Catalogue sync.
//!
//! The three sources are walked in a fixed order because they join into each
//! other: AniList defines the row set, Kitsu attaches ratings and external ids
//! through `mappings`, and Shikimori attaches Russian titles by name. Genres
//! and the search index are rebuilt once, at the end.
//!
//! The previous version started Kitsu and the genre matcher on `sleep(30)` and
//! `sleep(120)` timers relative to process start, which is a race: on a slow
//! or already-warm run the genre matcher saw a half-populated table and the
//! sleeps were pure guesswork. Sequencing with `await` removes the guesswork.

pub mod anilist;
pub mod genres;
pub mod kitsu;
pub mod kitsu_cast;
pub mod shikimori;

use crate::config::Config;
use crate::db::Handle;
use crate::error::{log_error, log_info};
use crate::sources::Sources;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// One import stage: a name for the log, and how to start it.
///
/// The future is boxed so every stage has the same type and the ordered run
/// below is a plain `for` loop instead of three hand-written awaits.
type Stage = (
    &'static str,
    fn(Ctx) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>,
);

/// Set to true to ask a running sync to stop after the current page.
pub static ABORT: AtomicBool = AtomicBool::new(false);

pub fn request_abort() {
    ABORT.store(true, Ordering::Relaxed);
}

pub fn clear_abort() {
    ABORT.store(false, Ordering::Relaxed);
}

fn aborted() -> bool {
    ABORT.load(Ordering::Relaxed)
}

#[derive(Clone)]
pub struct Ctx {
    pub db: Handle,
    pub sources: Arc<Sources>,
    pub cfg: Arc<Config>,
}

impl Ctx {
    pub fn abort_requested(&self) -> bool {
        aborted()
    }
}

/// Runs every enabled source to completion, then rebuilds derived data.
/// Safe to call again at any time: each stage resumes from its checkpoint.
pub async fn run_all(ctx: Ctx) {
    clear_abort();

    let started = std::time::Instant::now();
    log_info("=== синхронизация каталога ===");

    // A full import takes hours at the public rate limits, and the search index
    // was only rebuilt once at the very end, so anyone visiting during the
    // import got an empty result set for `q`. Rebuilding periodically makes
    // search usable while the catalogue is still filling up.
    let indexer = ctx.clone();
    let index_handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(120)).await;
            if indexer.abort_requested() {
                return;
            }
            if let Err(e) = rebuild_index(&indexer).await {
                log_error(&format!("[index] промежуточная пересборка: {}", e));
            }
        }
    });

    // The order is a dependency chain, not a preference: AniList defines the
    // row set, Kitsu attaches ratings and external ids through `mappings`, and
    // Shikimori attaches Russian titles by name.
    //
    // The boxed future is what lets the three sit in one array: each stage is an
    // `async fn`, and awaiting them through a uniform type is what turns "run
    // these in order" into a loop.
    let stages: [Stage; 3] = [
        ("anilist", |c| Box::pin(anilist::run(c))),
        ("kitsu", |c| Box::pin(kitsu::run(c))),
        ("shikimori", |c| Box::pin(shikimori::run(c))),
    ];

    for (name, stage) in stages {
        if let Err(e) = db_mark(&ctx, name).await {
            log_error(&format!("[{}] {}", name, e));
            continue;
        }
        if let Err(e) = stage(ctx.clone()).await {
            log_error(&format!("[{}] {}", name, e));
        }
        if ctx.abort_requested() {
            log_info(&format!(
                "=== синхронизация прервана по запросу на этапе {} ({} с) ===",
                name,
                started.elapsed().as_secs()
            ));
            index_handle.abort();
            return;
        }
    }

    // Cast and staff, after the catalogue so there is something to walk.
    if let Err(e) = kitsu_cast::run(ctx.clone(), ctx.cfg.kitsu_enrich_limit).await {
        log_error(&format!("[kitsu/cast] {}", e));
    }
    index_handle.abort();
    if ctx.abort_requested() {
        log_info("=== синхронизация прервана по запросу (дополнительные данные) ===");
        return;
    }

    // Derived tables only make sense once every source has landed.
    if let Err(e) = genres::match_all(ctx.clone()).await {
        log_error(&format!("[genres] {}", e));
    }
    if let Err(e) = rebuild_index(&ctx).await {
        log_error(&format!("[index] {}", e));
    }

    summarize(&ctx).await;
    log_info(&format!(
        "=== синхронизация завершена за {} ===",
        human_secs(started.elapsed().as_secs())
    ));
}

async fn db_mark(ctx: &Ctx, source: &str) -> Result<(), String> {
    let conn = ctx.db.conn().map_err(|e| e.to_string())?;
    crate::db::mark_started(&conn, source).map_err(|e| e.to_string())
}

async fn rebuild_index(ctx: &Ctx) -> Result<(), String> {
    if !ctx.db.fts_enabled() {
        return Ok(());
    }
    let db = ctx.db.clone();
    let n = tokio::task::spawn_blocking(move || {
        let conn = db.conn().map_err(|e| e.to_string())?;
        crate::db::rebuild_fts(&conn).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    log_info(&format!("[index] поисковый индекс: {} записей", n));
    Ok(())
}

async fn summarize(ctx: &Ctx) {
    let db = ctx.db.clone();
    let handle = tokio::task::spawn_blocking(move || {
        db.with(|c| -> Result<serde_json::Value, rusqlite::Error> {
            Ok(serde_json::json!({
                "anime": c.query_row("SELECT COUNT(*) FROM anime", [], |r| r.get::<_, i64>(0))?,
                "with_russian": c.query_row(
                    "SELECT COUNT(*) FROM anime WHERE title_russian IS NOT NULL AND title_russian <> ''",
                    [], |r| r.get::<_, i64>(0))?,
                "with_kitsu": c.query_row(
                    "SELECT COUNT(*) FROM anime WHERE kitsu_id IS NOT NULL", [], |r| r.get::<_, i64>(0))?,
                "with_anilist": c.query_row(
                    "SELECT COUNT(*) FROM anime WHERE anilist_id IS NOT NULL", [], |r| r.get::<_, i64>(0))?,
                "genres": c.query_row("SELECT COUNT(*) FROM genres", [], |r| r.get::<_, i64>(0))?,
            }))
        })
    });

    // `Handle::with` maps the rusqlite error into PoolError, so the awaited
    // join handle yields Result<Result<Value, PoolError>, JoinError>.
    let Ok(Ok(stats)) = handle.await else {
        return;
    };
    log_info(&format!(
        "[итог] всего {} | AniList {} | Kitsu {} | с русским названием {} | жанров {}",
        stats["anime"], stats["with_anilist"], stats["with_kitsu"], stats["with_russian"], stats["genres"]
    ));
}

pub fn human_secs(s: u64) -> String {
    if s < 60 {
        format!("{} с", s)
    } else if s < 3600 {
        format!("{} мин {} с", s / 60, s % 60)
    } else {
        format!("{} ч {} мин", s / 3600, (s % 3600) / 60)
    }
}

/// Lowercased, whitespace-collapsed key used for cross-source title matching.
pub fn title_key(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = true; // also trims the front
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
            prev_space = false;
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Turns the `<p>…</p><p>…</p>` fragments AniList and Kitsu use for
/// descriptions into plain text with paragraph breaks as newlines. The API
/// serves text, never HTML, so the markup must not reach a client.
pub fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    let mut tag = String::new();
    for ch in s.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' => {
                if in_tag {
                    in_tag = false;
                    let t = tag.trim().to_ascii_lowercase();
                    if matches!(t.as_str(), "br" | "br/" | "/p" | "p") {
                        out.push('\n');
                    }
                }
            }
            _ if in_tag => tag.push(ch),
            _ => out.push(ch),
        }
    }
    // Unescape the handful of entities the sources actually emit.
    let mut text = String::with_capacity(out.len());
    for line in out.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(t);
    }
    let text = text
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ");
    text.trim().to_string()
}
