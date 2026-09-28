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
    // Fingerprinted (e.g. app.a1b2c3.js) — safe to cache for a year.
    //
    // The fingerprint is the segment *before* the extension. An earlier version
    // tested the first segment instead, so `app.a1b2c3.js` never looked
    // fingerprinted (`app` has no digits) and every hashed asset was served
    // with `no-cache` — the exact outcome this function exists to avoid.
    let parts: Vec<&str> = name.split('.').collect();
    if let Some(fp) = parts.len().checked_sub(2).map(|i| parts[i]) {
        let looks_like_a_hash = fp.len() >= 6
            && fp.chars().any(|c| c.is_ascii_digit())
            && fp.chars().all(|c| c.is_ascii_alphanumeric());
        if looks_like_a_hash {
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

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::http::StatusCode;
    use actix_web::test::TestRequest;

    fn root() -> &'static Path {
        Path::new("/srv/frontend")
    }

    // ------------------------------------------------------------ traversal

    #[test]
    fn a_plain_relative_path_resolves_under_the_root() {
        let p = resolve(root(), "static/app.js").unwrap();
        assert_eq!(p, Path::new("/srv/frontend/static/app.js"));
    }

    #[test]
    fn a_leading_slash_is_not_a_component() {
        // The URL path always starts with '/', and treating it as absolute
        // would make every asset 404.
        assert_eq!(
            resolve(root(), "/index.html").unwrap(),
            Path::new("/srv/frontend/index.html")
        );
    }

    #[test]
    fn percent_escapes_are_decoded() {
        // A Russian filename arrives percent-encoded and must not be looked up
        // under its escaped name.
        assert_eq!(
            resolve(root(), "static/%D0%BA%D0%B0%D1%82.html").unwrap(),
            Path::new("/srv/frontend/static/кат.html")
        );
    }

    #[test]
    fn a_parent_component_is_refused() {
        // The whole point of the hand-written resolver: `..` is rejected, not
        // cleaned and then checked against a prefix.
        assert!(resolve(root(), "../secrets.txt").is_none());
        assert!(resolve(root(), "static/../../etc/passwd").is_none());
        assert!(resolve(root(), "a/b/../../../c").is_none());
    }

    #[test]
    fn an_encoded_parent_component_is_refused_too() {
        // Decoding happens before the component walk, so `%2e%2e` is just as
        // dangerous as `..` — and must be treated the same way.
        assert!(resolve(root(), "%2e%2e/secrets.txt").is_none());
        assert!(resolve(root(), "static/%2E%2E/%2E%2E/secret").is_none());
    }

    #[test]
    fn a_current_directory_component_is_ignored() {
        assert_eq!(
            resolve(root(), "./index.html").unwrap(),
            Path::new("/srv/frontend/index.html")
        );
    }

    #[test]
    fn the_root_itself_is_not_a_file() {
        // Returning the directory would let a directory listing out of a
        // handler that only knows how to read a file.
        assert!(resolve(root(), "").is_none());
        assert!(resolve(root(), "/").is_none());
        assert!(resolve(root(), ".").is_none());
    }

    #[test]
    fn a_sibling_with_a_shared_prefix_is_not_reachable() {
        // `/srv/app-secrets` starts with `/srv/app`, which is exactly the
        // mistake a string-prefix check makes.
        let p = resolve(Path::new("/srv/app"), "static/x.js").unwrap();
        assert_eq!(p, Path::new("/srv/app/static/x.js"));
        assert_ne!(p, Path::new("/srv/app-secrets/static/x.js"));
    }

    #[test]
    fn an_invalid_percent_sequence_is_refused() {
        // `%zz` is not a valid escape and is passed through as text, so it
        // resolves to a file that simply does not exist. A byte that is not
        // valid UTF-8 is what actually gets rejected.
        assert!(resolve(root(), "static/%FF.js").is_none());
    }

    // --------------------------------------------------------------- dates

    #[test]
    fn httpdate_renders_the_imf_fixdate() {
        // The only format Last-Modified accepts; anything else is a 400 from
        // the browser's cache.
        let t = UNIX_EPOCH + std::time::Duration::from_secs(784_111_777);
        assert_eq!(httpdate(&t), "Sun, 06 Nov 1994 08:49:37 GMT");
    }

    #[test]
    fn httpdate_of_the_epoch() {
        assert_eq!(httpdate(&UNIX_EPOCH), "Thu, 01 Jan 1970 00:00:00 GMT");
    }

    #[test]
    fn httpdate_handles_a_leap_day() {
        // The civil-from-days conversion has to know February can be 29 days
        // long, or every leap year is off by one.
        let t = UNIX_EPOCH + std::time::Duration::from_secs(1_709_164_800);
        assert_eq!(httpdate(&t), "Thu, 29 Feb 2024 00:00:00 GMT");
    }

    #[test]
    fn parse_httpdate_reads_what_httpdate_wrote() {
        // The two must be exact inverses, or every conditional request
        // mismatches and the file is re-downloaded every time.
        for secs in [0u64, 784_111_777, 1_709_164_800, 1_900_000_000] {
            let t = UNIX_EPOCH + std::time::Duration::from_secs(secs);
            let s = httpdate(&t);
            assert_eq!(parse_httpdate(&s), Some(secs), "дата {}", s);
        }
    }

    #[test]
    fn parse_httpdate_rejects_nonsense() {
        for s in [
            "",
            "not a date",
            "Sun 06 Nov 1994 08:49:37 GMT",
            "Sun, 06 Nov 1994",
            "Sun, 06 Xxx 1994 08:49:37 GMT",
            "Sun, 06 Nov",
        ] {
            assert_eq!(parse_httpdate(s), None, "дата {:?} разобралась", s);
        }
    }

    // -------------------------------------------------------- cache policy

    #[test]
    fn a_plain_asset_is_revalidated_every_time() {
        // Nothing in frontend/static is content-hashed, so a long max-age
        // would keep serving yesterday's bundle after a deploy.
        assert_eq!(cache_for(Path::new("/srv/f/static/app.js")), "public, no-cache");
        assert_eq!(cache_for(Path::new("/srv/f/index.html")), "public, no-cache");
    }

    #[test]
    fn a_fingerprinted_asset_is_immutable() {
        // The hash is the segment before the extension, not the first one.
        assert_eq!(
            cache_for(Path::new("/srv/f/static/app.a1b2c3.js")),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(
            cache_for(Path::new("/srv/f/static/vendor.1a2b3c4d.css")),
            "public, max-age=31536000, immutable"
        );
    }

    #[test]
    fn an_unhashed_multi_dot_name_is_still_revalidated() {
        // `main.min.js` and `jquery-3.6.0.min.js` are not fingerprinted, and
        // caching them for a year would keep a stale bundle alive after a
        // deploy.
        assert_eq!(cache_for(Path::new("/srv/f/static/main.min.js")), "public, no-cache");
        assert_eq!(
            cache_for(Path::new("/srv/f/static/jquery-3.6.0.min.js")),
            "public, no-cache"
        );
        assert_eq!(cache_for(Path::new("/srv/f/static/2.js")), "public, no-cache");
    }

    #[test]
    fn cache_for_never_panics_on_a_nameless_path() {
        assert_eq!(cache_for(Path::new("/")), "public, no-cache");
        assert_eq!(cache_for(Path::new("")), "public, no-cache");
    }

    // ------------------------------------------------------------- etags

    #[test]
    fn an_if_none_match_that_does_not_match_is_not_a_hit() {
        let req = TestRequest::default()
            .insert_header((header::IF_NONE_MATCH, "\"deadbeef-1\""))
            .to_http_request();
        assert!(!etag_matches(&req, "\"cafe-2\""));
    }

    #[test]
    fn a_star_always_matches() {
        let req = TestRequest::default()
            .insert_header((header::IF_NONE_MATCH, "*"))
            .to_http_request();
        assert!(etag_matches(&req, "\"cafe-2\""));
    }

    #[test]
    fn a_weak_comparison_matches() {
        // `W/` marks a semantically equal body; a weak comparison is what a
        // browser cache needs.
        let req = TestRequest::default()
            .insert_header((header::IF_NONE_MATCH, "W/\"cafe-2\""))
            .to_http_request();
        assert!(etag_matches(&req, "\"cafe-2\""));
    }

    #[test]
    fn one_matching_tag_among_several_is_a_hit() {
        let req = TestRequest::default()
            .insert_header((header::IF_NONE_MATCH, "\"other-1\", \"cafe-2\""))
            .to_http_request();
        assert!(etag_matches(&req, "\"cafe-2\""));
    }

    #[test]
    fn a_request_without_the_header_is_a_miss() {
        let req = TestRequest::default().to_http_request();
        assert!(!etag_matches(&req, "\"cafe-2\""));
    }

    // ------------------------------------------------------ serving a file

    #[tokio::test]
    async fn a_missing_file_is_a_plain_404() {
        let req = TestRequest::default().to_http_request();
        let res = serve(&req, Path::new("/definitely/not/here"), "index.html").await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn a_traversal_attempt_is_a_404_and_not_a_read() {
        let req = TestRequest::default().to_http_request();
        let res = serve(&req, root(), "../../../etc/passwd").await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn a_directory_is_not_served() {
        let req = TestRequest::default().to_http_request();
        let res = serve(&req, Path::new("."), "src").await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn a_real_file_is_served_with_its_type_cache_headers_and_etag() {
        // The four things the frontend depends on: a content type the browser
        // will render, a cache policy, a validator, and a body.
        let dir = std::env::temp_dir().join(format!("anime-static-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("app.js");
        std::fs::write(&file, b"console.log(1)").unwrap();

        let req = TestRequest::default().to_http_request();
        let res = serve(&req, &dir, "app.js").await;
        assert_eq!(res.status(), StatusCode::OK);
        let ctype = res.headers().get(header::CONTENT_TYPE).unwrap().to_str().unwrap();
        assert!(ctype.contains("javascript"), "content-type: {}", ctype);
        assert_eq!(res.headers().get(header::CACHE_CONTROL).unwrap(), "public, no-cache");
        let etag = res.headers().get(header::ETAG).unwrap().to_str().unwrap().to_string();
        assert!(etag.starts_with('"') && etag.ends_with('"'), "etag: {}", etag);
        assert!(res.headers().get(header::LAST_MODIFIED).is_some());

        // The same validator turns the next request into a 304 with no body.
        let req = TestRequest::default()
            .insert_header((header::IF_NONE_MATCH, etag))
            .to_http_request();
        let res = serve(&req, &dir, "app.js").await;
        assert_eq!(res.status(), StatusCode::NOT_MODIFIED);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_percent_encoded_request_reaches_a_cyrillic_file() {
        let dir = std::env::temp_dir().join(format!("anime-static-ru-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("каталог.txt"), b"ok").unwrap();

        let req = TestRequest::default().to_http_request();
        let res = serve(
            &req,
            &dir,
            "%D0%BA%D0%B0%D1%82%D0%B0%D0%BB%D0%BE%D0%B3.txt",
        )
        .await;
        assert_eq!(res.status(), StatusCode::OK);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_modified_since_in_the_past_still_serves_the_body() {
        let dir = std::env::temp_dir().join(format!("anime-static-dt-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("a.txt"), b"ok").unwrap();

        let req = TestRequest::default()
            .insert_header((header::IF_MODIFIED_SINCE, "Sun, 06 Nov 1994 08:49:37 GMT"))
            .to_http_request();
        let res = serve(&req, &dir, "a.txt").await;
        assert_eq!(res.status(), StatusCode::OK);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

