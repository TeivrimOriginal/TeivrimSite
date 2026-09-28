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
                } else {
                    // A `>` that closes nothing is text. Descriptions contain
                    // comparisons ("серия 3 > серия 2"), and dropping the
                    // character silently rewrote the sentence.
                    out.push('>');
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
    decode_entities(&text).trim().to_string()
}

/// Decodes HTML entities in one left-to-right pass.
///
/// A chain of `str::replace` calls cannot do this: it rewrites its own output,
/// so `&amp;lt;` becomes `<` instead of the `&lt;` the source actually sent.
/// Inventing markup is exactly what this function exists to prevent.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }

    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        // A named entity is at most six characters and a numeric one about ten.
        // Anything longer before a ';' is a bare ampersand, not an entity.
        let window = &tail[..tail.len().min(12)];
        match window.find(';') {
            Some(end) => {
                let name = &window[1..end];
                match decode_entity(name) {
                    Some(decoded) => out.push_str(&decoded),
                    None => {
                        out.push('&');
                        out.push_str(name);
                        out.push(';');
                    }
                }
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// One entity body, without the `&` or the `;`. `None` means "not an entity we
/// know", and the caller leaves the text as it found it.
///
/// The set is what the three sources actually emit. Numeric references are
/// handled generically below, which covers the long tail of typographic codes
/// without a table entry each.
fn decode_entity(body: &str) -> Option<String> {
    let named = match body {
        "quot" => "\"",
        "amp" => "&",
        "apos" => "'",
        "lt" => "<",
        "gt" => ">",
        "nbsp" => " ",
        "hellip" => "…",
        "mdash" => "—",
        "ndash" => "–",
        "lsquo" => "\u{2018}",
        "rsquo" => "\u{2019}",
        "ldquo" => "\u{201C}",
        "rdquo" => "\u{201D}",
        "laquo" => "«",
        "raquo" => "»",
        "middot" => "·",
        "bull" => "•",
        _ => return decode_numeric(body),
    };
    Some(named.to_string())
}

fn decode_numeric(body: &str) -> Option<String> {
    // &#NN; and &#xNN;. `from_u32` rejects the surrogate range, so a crafted
    // `&#xD800;` cannot smuggle an invalid code point through.
    let digits = body.strip_prefix('#')?;
    let code = match digits.strip_prefix('x').or_else(|| digits.strip_prefix('X')) {
        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
        None => digits.parse::<u32>().ok()?,
    };
    char::from_u32(code).map(|c| c.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------ title_key

    #[test]
    fn title_key_lowercases_and_collapses_whitespace() {
        assert_eq!(title_key("Shingeki no Kyojin"), "shingeki no kyojin");
        assert_eq!(title_key("a   b"), "a b");
        assert_eq!(title_key("a\t\nb"), "a b");
    }

    #[test]
    fn title_key_trims_both_ends() {
        // `prev_space` starts as true, which is what trims the front; the loop
        // at the end handles the back.
        assert_eq!(title_key("   "), "");
        assert_eq!(title_key("\t\n Frieren \r\n"), "frieren");
    }

    #[test]
    fn title_key_is_idempotent() {
        // The key is compared against a stored key during the Shikimori join, so
        // key(key(x)) == key(x) is what makes repeated normalisation safe.
        for s in [
            "Shingeki no Kyojin",
            "Атака Титанов",
            "  spaced  out  ",
            "Re:Zero",
            "",
            "Straße",
            "İstanbul",
        ] {
            let once = title_key(s);
            assert_eq!(title_key(&once), once, "not idempotent for {:?}", s);
        }
    }

    #[test]
    fn title_key_folds_case_for_cyrillic() {
        // This is the whole reason the join uses a computed key rather than the
        // raw title: SQLite's LIKE folds ASCII only.
        assert_eq!(title_key("АТАКА ТИТАНОВ"), title_key("Атака Титанов"));
        assert_ne!(title_key("АТАКА"), "АТАКА");
    }

    #[test]
    fn title_key_keeps_punctuation() {
        // Documented limitation, not an oversight: `Re:Zero` and `Re Zero` do
        // not join. Punctuation is significant in too many titles that folding it
        // would collide on pairs like "Re:Zero" / "Re Zero" being *different*
        // shows.
        assert_eq!(title_key("Re:Zero"), "re:zero");
        assert_ne!(title_key("Re:Zero"), title_key("Re Zero"));
    }

    #[test]
    fn title_key_handles_german_sharp_s() {
        // 'ß'.to_lowercase() is 'ß', not 'ss' — a note so the behaviour is not
        // mistaken for a bug when a German title fails to join.
        assert_eq!(title_key("STRAßE"), "straße");
    }

    // ------------------------------------------------------------ strip_html

    #[test]
    fn strip_html_turns_paragraphs_into_lines() {
        assert_eq!(strip_html("<p>One</p><p>Two</p>"), "One\nTwo");
        assert_eq!(strip_html("Line<br>Next"), "Line\nNext");
        assert_eq!(strip_html("<p>A</p><br/><p>B</p>"), "A\nB");
    }

    #[test]
    fn strip_html_drops_tags_but_keeps_the_text_between_them() {
        assert_eq!(strip_html("a <b>bold</b> c"), "a bold c");
        assert_eq!(strip_html("<span>only</span>"), "only");
        assert_eq!(strip_html("<i><u>deep</u></i>"), "deep");
    }

    #[test]
    fn strip_html_keeps_line_structure_but_drops_blank_lines() {
        assert_eq!(strip_html("<p>One</p><p></p><p>Two</p>"), "One\nTwo");
    }

    #[test]
    fn strip_html_unescapes_the_entities_the_sources_emit() {
        assert_eq!(strip_html("Tom &amp; Jerry"), "Tom & Jerry");
        assert_eq!(strip_html("&quot;quoted&quot;"), "\"quoted\"");
        assert_eq!(strip_html("&apos;x&apos;"), "'x'");
        assert_eq!(strip_html("&lt;tag&gt;"), "<tag>");
        assert_eq!(strip_html("a&nbsp;b"), "a b");
    }

    #[test]
    fn strip_html_decodes_an_entity_exactly_once() {
        // A chain of `.replace` calls decodes twice: "&amp;lt;" went through
        // `&amp;` to become "&lt;" and then through `&lt;` to become "<", which
        // invents markup the source never sent. A single left-to-right pass is
        // the only way to keep the mapping one-to-one.
        assert_eq!(strip_html("&amp;lt;"), "&lt;");
        assert_eq!(strip_html("&amp;amp;"), "&amp;");
        // The same has to hold when the literal is inside a paragraph.
        assert_eq!(strip_html("<p>&amp;lt;script&amp;gt;</p>"), "&lt;script&gt;");
    }

    #[test]
    fn strip_html_decodes_numeric_references() {
        // The long tail of typographic codes would need a table entry each, so
        // they are handled numerically instead.
        assert_eq!(strip_html("&#39;x&#39;"), "'x'");
        assert_eq!(strip_html("a&#8212;b"), "a—b");
        assert_eq!(strip_html("&#x2014;"), "—");
    }

    #[test]
    fn strip_html_decodes_the_typographic_named_entities() {
        assert_eq!(strip_html("a&hellip;b"), "a…b");
        assert_eq!(strip_html("a&mdash;b"), "a—b");
        assert_eq!(strip_html("&ldquo;hi&rdquo;"), "“hi”");
        assert_eq!(strip_html("&laquo;hi&raquo;"), "«hi»");
    }

    #[test]
    fn strip_html_leaves_an_unknown_named_entity_alone() {
        // Nothing is lost by passing it through: the client renders text, and a
        // literal `&copy;` is better than a silently dropped character.
        assert_eq!(strip_html("&copy; 2024"), "&copy; 2024");
    }

    #[test]
    fn strip_html_leaves_a_bare_ampersand_alone() {
        assert_eq!(strip_html("Tom & Jerry"), "Tom & Jerry");
        assert_eq!(strip_html("a & b"), "a & b");
        assert_eq!(strip_html("&notanentity; x"), "&notanentity; x");
    }

    #[test]
    fn strip_html_survives_malformed_markup() {
        // These sources return broken HTML regularly. This is a state machine,
        // not a parser, and it has to terminate and stay sane on anything.
        // An unterminated tag swallows the rest, which is what any
        // tag-stripper does and is the safe direction to fail in. A stray `>`
        // is text, though, and has to survive.
        assert_eq!(strip_html("text <b"), "text");
        assert_eq!(strip_html("<"), "");
        assert_eq!(strip_html("a > b"), "a > b");
        assert_eq!(strip_html("<<>>"), ">");
        assert_eq!(strip_html("серия 3 > серия 2"), "серия 3 > серия 2");
    }

    // ----------------------------------------------------------- human_secs

    #[test]
    fn humansecs_picks_the_largest_sensible_unit() {
        assert_eq!(human_secs(0), "0 с");
        assert_eq!(human_secs(59), "59 с");
        assert_eq!(human_secs(60), "1 мин 0 с");
        assert_eq!(human_secs(3599), "59 мин 59 с");
        assert_eq!(human_secs(3600), "1 ч 0 мин");
        assert_eq!(human_secs(7384), "2 ч 3 мин");
    }
}
