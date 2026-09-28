//! Image proxy.
//!
//! Cover art lives on a handful of third-party CDNs. Serving it directly from
//! the client means a Russian visitor hits those hosts for every one of the ~50
//! covers on screen, and the app depends on hosts that rate-limit and change
//! URLs without notice. Proxying gives one cacheable endpoint, one place to
//! enforce an allow-list, and a place to serve a generated placeholder when an
//! upstream image is missing.

use crate::error::{log_warn, ApiError, ApiResult};
use actix_web::{web, HttpRequest, HttpResponse, ResponseError};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Only these hosts may be proxied. Without this the endpoint would be an open
/// relay that anything on the internet could use to reach internal addresses.
const ALLOWED_HOSTS: &[&str] = &[
    "s4.anilist.co",
    "s1.anilist.co",
    "s2.anilist.co",
    "s3.anilist.co",
    "img.anili.st",
    "media.kitsu.app",
    "media.kitsu.io",
    "shikimori.one",
    "cdn.anili.st",
];

const MAX_BYTES: usize = 8 * 1024 * 1024;
const CACHE_TTL: Duration = Duration::from_secs(60 * 60 * 24 * 3);
const CACHE_MAX_ENTRIES: usize = 2_000;

struct Cached {
    body: Vec<u8>,
    content_type: String,
    stored: Instant,
}

pub struct ImageCache(Mutex<HashMap<String, Cached>>);

impl Default for ImageCache {
    fn default() -> Self {
        ImageCache(Mutex::new(HashMap::new()))
    }
}

/// A 2:3 grey card with a glyph, so a missing cover still respects the grid.
const PLACEHOLDER: &[u8] = b"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 200 300'><rect width='200' height='300' fill='#1b1b20'/><path d='M70 128l30-30 30 30-30 30z' fill='#3a3a45'/><circle cx='100' cy='185' r='14' fill='#3a3a45'/></svg>";

pub async fn proxy(
    cache: web::Data<ImageCache>,
    limiter: web::Data<crate::http::ratelimit::Limiter>,
    req: HttpRequest,
    q: web::Query<std::collections::HashMap<String, String>>,
) -> HttpResponse {
    // A page shows ~50 covers, so the budget has to be generous for a human
    // scrolling but still bounded against someone using it as a hotlink relay.
    let key = crate::http::client_key_from(&req);
    let decision = limiter.check_n(&key, 240);
    if !decision.allowed {
        return HttpResponse::TooManyRequests()
            .insert_header(("retry-after", decision.retry_after.to_string()))
            .content_type("image/svg+xml")
            .body(PLACEHOLDER.to_vec());
    }

    let Some(raw) = q.get("u") else {
        return ApiError::bad("параметр u обязателен").error_response();
    };
    if raw.len() > 2048 {
        return ApiError::bad("слишком длинный URL").error_response();
    }

    let target = match parse_and_authorize(raw) {
        Ok(t) => t,
        Err(e) => return e.error_response(),
    };

    // Only cache successful GETs; anything else is re-fetched next time.
    if let Some(hit) = cache.get(&target) {
        return build(&hit.body, &hit.content_type);
    }

    let client = match client() {
        Ok(c) => c,
        Err(e) => return e.error_response(),
    };

    let response = match client.get(&target).send().await {
        Ok(r) => r,
        Err(e) => {
            log_warn(&format!("[img] не удалось получить {}: {}", target, e));
            return placeholder();
        }
    };

    if !response.status().is_success() {
        log_warn(&format!("[img] {} -> HTTP {}", target, response.status()));
        return placeholder();
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .split(';')
        .next()
        .unwrap_or("image/jpeg")
        .trim()
        .to_string();

    // Only image content types are stored, so a redirect to HTML cannot be
    // served back as an image.
    if !content_type.starts_with("image/") {
        return placeholder();
    }

    let bytes = match response.bytes().await {
        Ok(b) => b,
        Err(e) => {
            log_warn(&format!("[img] чтение тела {}: {}", target, e));
            return placeholder();
        }
    };
    if bytes.len() > MAX_BYTES {
        log_warn(&format!("[img] {} слишком большой ({} Б)", target, bytes.len()));
        return placeholder();
    }

    let body = bytes.to_vec();
    cache.put(&target, body.clone(), content_type.clone());
    build(&body, &content_type)
}

fn build(body: &[u8], content_type: &str) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header((actix_web::http::header::CONTENT_TYPE, content_type))
        // Immutable in practice: a given URL keeps pointing at the same image,
        // and a wrong cover is fixed by a new URL, not by mutating this one.
        .insert_header((
            actix_web::http::header::CACHE_CONTROL,
            "public, max-age=604800, immutable",
        ))
        .body(body.to_vec())
}

fn placeholder() -> HttpResponse {
    HttpResponse::Ok()
        .insert_header((
            actix_web::http::header::CONTENT_TYPE,
            "image/svg+xml; charset=utf-8",
        ))
        .insert_header((
            actix_web::http::header::CACHE_CONTROL,
            "public, max-age=3600",
        ))
        .body(PLACEHOLDER.to_vec())
}

fn parse_and_authorize(raw: &str) -> ApiResult<String> {
    let decoded = percent_encoding::percent_decode_str(raw)
        .decode_utf8()
        .map_err(|_| ApiError::bad("некорректный URL"))?
        .into_owned();

    let rest = decoded
        .strip_prefix("https://")
        .or_else(|| decoded.strip_prefix("http://"))
        .ok_or_else(|| ApiError::bad("поддерживаются только http(s) URL"))?;

    let host_end = rest.find('/').unwrap_or(rest.len());
    let host = rest[..host_end].to_ascii_lowercase();
    let host = host.split('@').next_back().unwrap_or(&host);
    let host = host.split(':').next().unwrap_or(host);

    if !ALLOWED_HOSTS.contains(&host) {
        // Generic message: do not hand the caller the allow-list.
        return Err(ApiError::bad("источник изображений не разрешён"));
    }
    Ok(decoded)
}

fn client() -> ApiResult<reqwest::Client> {
    use std::time::Duration as D;
    reqwest::Client::builder()
        .timeout(D::from_secs(12))
        .connect_timeout(D::from_secs(5))
        .user_agent("anime-db/2.0 (+https://github.com/TeivrimOriginal/TeivrimSite)")
        .build()
        .map_err(|e| {
            log_warn(&format!("[img] не удалось создать клиент: {}", e));
            ApiError::internal("image client unavailable")
        })
}

impl ImageCache {
    fn get(&self, key: &str) -> Option<Cached> {
        let mut map = self.0.lock().unwrap_or_else(|p| p.into_inner());
        let hit = map.get(key)?;
        if hit.stored.elapsed() > CACHE_TTL {
            map.remove(key);
            return None;
        }
        Some(Cached {
            body: hit.body.clone(),
            content_type: hit.content_type.clone(),
            stored: hit.stored,
        })
    }

    fn put(&self, key: &str, body: Vec<u8>, content_type: String) {
        let mut map = self.0.lock().unwrap_or_else(|p| p.into_inner());
        if map.len() >= CACHE_MAX_ENTRIES {
            // Cheap eviction: drop the oldest quarter. A precise LRU is not
            // worth the bookkeeping for a cache this size.
            let drop_count = CACHE_MAX_ENTRIES / 4;
            let mut victims: Vec<(String, Instant)> = map
                .iter()
                .map(|(k, v)| (k.clone(), v.stored))
                .collect();
            victims.sort_by_key(|(_, t)| *t);
            for (k, _) in victims.into_iter().take(drop_count) {
                map.remove(&k);
            }
        }
        map.insert(
            key.to_string(),
            Cached {
                body,
                content_type,
                stored: Instant::now(),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed(raw: &str) -> String {
        parse_and_authorize(raw).unwrap_or_else(|e| panic!("{} отклонён: {}", raw, e))
    }

    fn rejected(raw: &str) {
        match parse_and_authorize(raw) {
            Ok(v) => panic!("{} не должен был пройти (получено {})", raw, v),
            // The message must not enumerate the allow-list.
            Err(e) => assert!(!e.to_string().contains("anilist.co"), "утечка списка хостов"),
        }
    }

    #[test]
    fn the_known_cdns_are_allowed() {
        for host in ["s4.anilist.co", "s1.anilist.co", "img.anili.st", "media.kitsu.app", "shikimori.one"] {
            let url = format!("https://{}/cover.jpg", host);
            assert_eq!(allowed(&url), url);
        }
    }

    #[test]
    fn a_port_in_the_url_is_tolerated() {
        // Some CDNs are reached on a non-default port in a lab setup; the host
        // is what is authorised, not the whole origin.
        let url = "https://media.kitsu.app:443/cover.jpg";
        assert_eq!(allowed(url), url);
    }

    #[test]
    fn the_host_is_matched_case_insensitively() {
        // The frontend sometimes upper-cases a host while building a URL.
        assert!(allowed("https://S4.ANILIST.CO/cover.jpg").starts_with("https://S4"));
    }

    #[test]
    fn an_arbitrary_host_is_refused() {
        // Without this the endpoint is an open relay into the deployment's
        // own network.
        rejected("https://evil.example/cover.jpg");
        rejected("https://localhost:8080/");
        rejected("https://169.254.169.254/latest/meta-data/");
        rejected("https://127.0.0.1/admin");
    }

    #[test]
    fn a_userinfo_trick_cannot_hide_the_real_host() {
        // `https://s4.anilist.co@evil.example/` is served by evil.example, and
        // a naive `split('@')` on the wrong side gets it exactly backwards.
        rejected("https://s4.anilist.co@evil.example/cover.jpg");
    }

    #[test]
    fn a_lookalike_host_is_refused() {
        rejected("https://s4.anilist.co.evil.example/x.jpg");
        rejected("https://evil-s4.anilist.co.example/x.jpg");
        rejected("https://nots4.anilist.co/x.jpg");
    }

    #[test]
    fn a_non_http_scheme_is_refused() {
        // `file://` and `gopher://` are not what an <img> can load anyway.
        rejected("file:///etc/passwd");
        rejected("ftp://s4.anilist.co/x.jpg");
        rejected("javascript:alert(1)");
        rejected("/static/cover.jpg");
        rejected("s4.anilist.co/cover.jpg");
    }

    #[test]
    fn a_percent_encoded_url_is_decoded_before_the_check() {
        // The client percent-encodes the whole target, so the check has to see
        // the decoded host or it authorises nothing at all.
        assert!(allowed("https%3A%2F%2Fs4.anilist.co%2Fcover.jpg").contains("s4.anilist.co"));
        rejected("https%3A%2F%2Fevil.example%2Fx.jpg");
    }

    #[test]
    fn a_percent_encoded_scheme_is_still_checked() {
        // `%68ttps://` decodes to `https://`; the check happens after decoding,
        // so the result is the same as the plain form.
        assert!(allowed("%68ttps://s4.anilist.co/x.jpg").contains("s4.anilist.co"));
    }

    #[test]
    fn a_url_without_a_path_is_still_a_valid_target() {
        assert_eq!(allowed("https://shikimori.one"), "https://shikimori.one");
    }

    // ---------------------------------------------------------- the cache

    #[test]
    fn the_cache_round_trips_a_body() {
        let c = ImageCache::default();
        assert!(c.get("k").is_none());
        c.put("k", b"bytes".to_vec(), "image/png".to_string());
        let hit = c.get("k").unwrap();
        assert_eq!(hit.body, b"bytes".to_vec());
        assert_eq!(hit.content_type, "image/png");
    }

    #[test]
    fn the_cache_forgets_a_stale_entry() {
        // A cover that changed upstream must not be served for three days.
        let c = ImageCache::default();
        c.put("k", b"old".to_vec(), "image/jpeg".to_string());
        {
            let mut map = c.0.lock().unwrap();
            let entry = map.get_mut("k").unwrap();
            entry.stored = Instant::now() - CACHE_TTL - std::time::Duration::from_secs(1);
        }
        assert!(c.get("k").is_none());
        let map = c.0.lock().unwrap();
        assert!(!map.contains_key("k"), "протухшая запись должна быть удалена");
    }

    #[test]
    fn the_cache_evicts_when_it_is_full() {
        // A precise LRU is not worth the bookkeeping, but the map must still
        // stay bounded.
        let c = ImageCache::default();
        for i in 0..CACHE_MAX_ENTRIES + 10 {
            c.put(&format!("k{}", i), vec![0u8; 1], "image/jpeg".to_string());
        }
        let map = c.0.lock().unwrap();
        assert!(
            map.len() <= CACHE_MAX_ENTRIES,
            "кэш вырос до {} записей",
            map.len()
        );
    }

    #[test]
    fn the_placeholder_is_a_valid_svg() {
        // It is served with an image content type, so it has to be one.
        let s = std::str::from_utf8(PLACEHOLDER).unwrap();
        assert!(s.starts_with("<svg"));
        assert!(s.ends_with("</svg>"));
    }
}
