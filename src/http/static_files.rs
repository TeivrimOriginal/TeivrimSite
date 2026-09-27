//! Static file serving for the frontend.
//!
//! A hand-rolled handler instead of `actix-files`, mainly so path traversal is
//! impossible by construction: the requested path is resolved component by
//! component against the root and any `..`, absolute, or Windows-prefix
//! component is rejected outright. The usual alternative — checking that the
//! cleaned path starts with the root string — is easy to get subtly wrong
//! (`/srv/app-secrets` starts with `/srv/app`).

use actix_web::{http::header, HttpRequest, HttpResponse};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Serves `rel` from `root`, honouring `If-None-Match` / `If-Modified-Since`.
pub async fn serve(req: &HttpRequest, root: &Path, rel: &str) -> HttpResponse {
    let Some(path) = resolve(root, rel) else {
        return not_found();
    };

    let Ok(meta) = tokio::fs::metadata(&path).await else {
        return not_found();
    };
    if !meta.is_file() {
        return not_found();
    }

    let etag = etag_of(&meta);
    let modified = meta.modified().ok();

    if let Some(m) = modified {
        if let Some(since) = req
            .headers()
            .get(header::IF_MODIFIED_SINCE)
            .and_then(|v| v.to_str().ok())
        {
            if let Some(t) = parse_httpdate(since) {
                if let Ok(actual) = m.duration_since(UNIX_EPOCH) {
                    // HTTP dates have one-second resolution, so compare
                    // truncated to seconds to avoid a permanent mismatch.
                    if actual.as_secs() == t {
                        return not_modified(&etag, modified);
                    }
                }
            }
        }
    }
    if etag_matches(req, &etag) {
        return not_modified(&etag, modified);
    }

    let data = match tokio::fs::read(&path).await {
        Ok(d) => d,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .content_type("text/plain; charset=utf-8")
                .body(format!("Не удалось прочитать файл: {}", e));
        }
    };

    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    let mut res = HttpResponse::Ok();
    res.insert_header((header::CONTENT_TYPE, mime.to_string()));
    res.insert_header((header::CACHE_CONTROL, cache_for(&path)));
    res.insert_header((header::ETAG, etag));
    if let Some(m) = modified {
        res.insert_header((header::LAST_MODIFIED, httpdate(&m)));
    }
    res.body(data)
}

fn not_modified(etag: &str, modified: Option<SystemTime>) -> HttpResponse {
    let mut res = HttpResponse::NotModified();
    res.insert_header((header::ETAG, etag.to_string()));
    if let Some(m) = modified {
        res.insert_header((header::LAST_MODIFIED, httpdate(&m)));
    }
    res.finish()
}

fn etag_matches(req: &HttpRequest, etag: &str) -> bool {
    let Some(header) = req.headers().get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    // `*` matches anything, and a weak comparison is what a cache needs here.
    header.split(',').map(|s| s.trim()).any(|t| t == "*" || t.trim_start_matches("W/") == etag)
}

/// Resolves a URL path under `root`, or `None` if it would escape.
pub fn resolve(root: &Path, rel: &str) -> Option<PathBuf> {
    let decoded = percent_encoding::percent_decode_str(rel.trim_start_matches('/'))
        .decode_utf8()
        .ok()?
        .into_owned();

    let mut out = root.to_path_buf();
    for comp in Path::new(&decoded).components() {
        match comp {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if out == *root {
        return None;
    }
    Some(out)
}

/// mtime + size. Assets are replaced wholesale on deploy, so this is enough
/// and avoids reading the file to hash it.
fn etag_of(meta: &std::fs::Metadata) -> String {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("\"{:x}-{:x}\"", mtime, meta.len())
}

/// Cache policy.
///
/// Nothing in `frontend/static/` is content-hashed, so a long `max-age` would
/// keep serving yesterday's bundle after a deploy: the browser would not even
/// revalidate. `no-cache` plus a strong ETag is the correct choice here — the
/// body is fetched only when the validator actually changed, which costs a 304
/// rather than a full download.
///
/// If fingerprinted filenames are introduced later, `/^\d/`, then the hashed
/// branch below becomes the right one for them and this can be narrowed.
fn cache_for(path: &Path) -> &'static str {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.contains('.') {
        let stem = name.split('.').next().unwrap_or("");
        // Fingerprinted (e.g. app.a1b2c3.js) — safe to cache for a year.
        if stem.chars().any(|c| c.is_ascii_digit()) && name.contains('.') && name.rsplit('.').count() >= 3 {
            return "public, max-age=31536000, immutable";
        }
    }
    "public, no-cache"
}

fn not_found() -> HttpResponse {
    HttpResponse::NotFound()
        .content_type("text/plain; charset=utf-8")
        .body("404 — не найдено")
}

const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Civil date from a Unix day number (Howard Hinnant's algorithm).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}

/// RFC 7231 IMF-fixdate, the only format `Last-Modified` accepts.
pub fn httpdate(t: &SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (y, m, d) = civil(days);
    format!(
        "{}, {:02} {} {} {:02}:{:02}:{:02} GMT",
        DAYS[(days.rem_euclid(7)) as usize],
        d,
        MONTHS[(m - 1) as usize],
        y,
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

/// Parses an IMF-fixdate. Returns the Unix seconds it represents.
fn parse_httpdate(s: &str) -> Option<u64> {
    // "Sun, 06 Nov 1994 08:49:37 GMT"
    let s = s.trim();
    let (_, rest) = s.split_once(", ")?;
    let mut it = rest.split(' ');
    let d: u32 = it.next()?.parse().ok()?;
    let mon = it.next()?;
    let y: i64 = it.next()?.parse().ok()?;
    let time = it.next()?;
    let mut tp = time.split(':');
    let h: i64 = tp.next()?.parse().ok()?;
    let mi: i64 = tp.next()?.parse().ok()?;
    let se: i64 = tp.next()?.parse().ok()?;
    let m = MONTHS.iter().position(|x| *x == mon)? as i64 + 1;

    // days since epoch for a civil date
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86_400 + h * 3600 + mi * 60 + se) as u64)
}

