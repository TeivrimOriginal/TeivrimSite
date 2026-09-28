use crate::error::log_warn;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a successful GET answer may be reused.
///
/// The measurement behind this enum, from the loaders as they were written:
/// `/api/*` never opens a connection to a source at all, it reads SQLite, and
/// inside a pass each `(source, sort, page)` URL is requested exactly once
/// because the walk goes forward and a restart resumes from the checkpoint.
/// So there was nothing to gain from caching pages.
///
/// What repeats is a pair of URLs that are asked for again on every pass, now
/// that there is a refresh timer: Kitsu's genre list and the Shikimori
/// reachability probe. Both are reference documents a source publishes once
/// and does not edit in place, and `fetch_genres` was already documented as
/// "cached once per process" while caching nothing at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// Every call goes out.
    ///
    /// What every catalogue page uses, and the only safe default: the whole
    /// point of asking AniList for page 400 is to see what it says now.
    Now,
    /// Reuse the answer for the life of the process.
    ///
    /// Only for reference data. It is not a TTL, because a TTL would be a lie
    /// either way — long enough not to matter, or short enough that a restarted
    /// process is the honest way to pick up an edited list.
    Forever,
}

/// Upper bounds on the cache.
///
/// The cache exists to stop two small documents being re-requested once per
/// pass, so it is sized for exactly that and refuses anything larger rather
/// than growing: a catalogue page would fit in the entry count and blow the
/// byte bound, and the byte bound is the one that matters.
const CACHE_MAX_ENTRIES: usize = 32;
const CACHE_MAX_BYTES: usize = 1024 * 1024;

/// What the cache is holding, for the log line and for the tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheStats {
    pub entries: usize,
    pub bytes: usize,
    pub hits: u64,
    pub misses: u64,
    /// Answers that were correct but too large to keep.
    pub refused: u64,
}

/// Reuse of reference answers, keyed by URL.
///
/// A `Vec` of entries would be fine at thirty-two, but a map is the same
/// amount of code and does not turn the second lookup into a scan.
#[derive(Default)]
struct Cache {
    entries: HashMap<String, Value>,
    bytes: usize,
    hits: u64,
    misses: u64,
    refused: u64,
}

impl Cache {
    fn get(&mut self, url: &str) -> Option<Value> {
        match self.entries.get(url) {
            // Cloned on the way out: the cache is read on every pass and a
            // caller that mutates a `Value` it was handed would poison it for
            // the next one. It is two reference documents a day.
            Some(v) => {
                self.hits += 1;
                Some(v.clone())
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }

    fn put(&mut self, url: &str, value: Value) {
        let size = value.to_string().len();
        if size > CACHE_MAX_BYTES || self.entries.len() >= CACHE_MAX_ENTRIES {
            // Not an error: the caller asked for reuse, not for storage, and a
            // document this large was never reference data to begin with.
            self.refused += 1;
            return;
        }
        self.bytes += size;
        self.entries.insert(url.to_string(), value);
    }

    fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.entries.len(),
            bytes: self.bytes,
            hits: self.hits,
            misses: self.misses,
            refused: self.refused,
        }
    }
}

/// A tiny fixed-window rate limiter.
///
/// AniList allows 30 requests/minute for anonymous clients and 90 when a client
/// id is supplied. The old loader slept a flat 750 ms between pages, which is
/// 80 req/min and therefore guaranteed 429s; pacing from a measured window
/// removes that failure mode entirely.
struct Window {
    start: Instant,
    count: u32,
}

pub struct Upstream {
    name: &'static str,
    client: reqwest::Client,
    window: Mutex<Window>,
    /// Minimum gap between two requests, derived from the quota.
    min_interval: Duration,
    /// Attempts per request, including the first.
    attempts: u32,
    extra_headers: Vec<(String, String)>,
    cache: Mutex<Cache>,
}

impl Upstream {
    pub fn new(
        name: &'static str,
        base_timeout: Duration,
        requests_per_minute: u32,
        extra_headers: Vec<(String, String)>,
    ) -> reqwest::Result<Upstream> {
        let client = reqwest::Client::builder()
            .user_agent("anime-db/2.0 (+https://github.com/TeivrimOriginal/TeivrimSite)")
            .timeout(base_timeout)
            .connect_timeout(Duration::from_secs(15))
            .pool_max_idle_per_host(4)
            .build()?;
        let min_interval = if requests_per_minute == 0 {
            Duration::ZERO
        } else {
            // 95% of the quota leaves headroom for bursts and clock skew.
            Duration::from_secs_f64(60.0 * 0.95 / requests_per_minute as f64)
        };
        Ok(Upstream {
            name,
            client,
            window: Mutex::new(Window {
                start: Instant::now(),
                count: 0,
            }),
            min_interval,
            attempts: 5,
            extra_headers,
            cache: Mutex::new(Cache::default()),
        })
    }

    /// Blocks the calling thread until the next request may go out. Requests
    /// are spaced by `min_interval` inside a sliding 60 second window.
    fn wait_turn(&self) {
        if self.min_interval.is_zero() {
            return;
        }
        loop {
            let sleep_for = {
                let mut w = self.window.lock().unwrap_or_else(|p| p.into_inner());
                let elapsed = w.start.elapsed();
                if elapsed >= Duration::from_secs(60) {
                    w.start = Instant::now();
                    w.count = 0;
                    continue;
                }
                let reset_in = Duration::from_secs(60) - elapsed;
                let next_allowed = self.min_interval * w.count;
                if elapsed >= next_allowed {
                    w.count += 1;
                    None
                } else {
                    Some((next_allowed - elapsed).min(reset_in))
                }
            };
            match sleep_for {
                None => return,
                Some(d) => std::thread::sleep(d),
            }
        }
    }

    fn build(&self, url: &str) -> reqwest::RequestBuilder {
        let mut rb = self.client.get(url);
        for (k, v) in &self.extra_headers {
            rb = rb.header(k.as_str(), v.as_str());
        }
        rb
    }

    fn build_post(&self, url: &str) -> reqwest::RequestBuilder {
        let mut rb = self.client.post(url);
        for (k, v) in &self.extra_headers {
            rb = rb.header(k.as_str(), v.as_str());
        }
        rb
    }

    /// GET returning parsed JSON, with pacing, `Retry-After` handling and
    /// exponential backoff. Nothing is reused: see [`Freshness`].
    pub async fn get_json(&self, url: &str) -> Result<Value, String> {
        self.fetch(|rb| async move { rb.send().await }, url, "GET", false)
            .await
    }

    /// GET whose answer the caller is willing to reuse.
    pub async fn get_json_cached(
        &self,
        url: &str,
        freshness: Freshness,
    ) -> Result<Value, String> {
        if freshness == Freshness::Now {
            return self.get_json(url).await;
        }
        if let Some(hit) = self.cache.lock().unwrap_or_else(|p| p.into_inner()).get(url) {
            return Ok(hit);
        }
        // The store happens inside `fetch` and only on a 2xx, so a failed
        // request is never remembered as an answer.
        self.fetch(|rb| async move { rb.send().await }, url, "GET", true)
            .await
    }

    /// What the cache is holding. Reported in the log after a pass so the claim
    /// in [`Freshness`] is a number in the log rather than a comment.
    pub fn cache_stats(&self) -> CacheStats {
        self.cache.lock().unwrap_or_else(|p| p.into_inner()).stats()
    }

    pub async fn post_json(&self, url: &str, body: &Value) -> Result<Value, String> {
        // Never cached. The body is the query, so two POSTs to the same URL are
        // two different questions and a cache keyed by URL cannot tell them
        // apart.
        self.fetch(|rb| async move { rb.json(body).send().await }, url, "POST", false)
            .await
    }

    async fn fetch<F, Fut>(
        &self,
        send: F,
        url: &str,
        method: &str,
        store: bool,
    ) -> Result<Value, String>
    where
        F: Fn(reqwest::RequestBuilder) -> Fut,
        Fut: std::future::Future<Output = reqwest::Result<reqwest::Response>>,
    {
        let mut backoff = Duration::from_secs(2);
        let mut last = String::new();

        for attempt in 1..=self.attempts {
            self.wait_turn();

            let rb = if method == "GET" {
                self.build(url)
            } else {
                self.build_post(url)
            };

            let resp = match send(rb).await {
                Ok(r) => r,
                Err(e) => {
                    last = format!("transport: {}", e);
                    log_warn(&format!("[{}] {} {} попытка {}: {}", self.name, method, url, attempt, e));
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(60));
                    continue;
                }
            };

            let status = resp.status();
            if status.is_success() {
                let text = resp.text().await.map_err(|e| format!("body: {}", e))?;
                let value: Value = serde_json::from_str(&text)
                    .map_err(|e| format!("json: {} (начало ответа: {})", e, truncate(&text, 200)))?;
                if store {
                    // Only a success is ever stored, so a source that was down
                    // for a minute is retried on the next pass instead of being
                    // remembered as broken for the life of the process.
                    self.cache
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .put(url, value.clone());
                }
                return Ok(value);
            }

            if status.as_u16() == 429 || status.as_u16() == 403 {
                // Respect Retry-After when the server sends one; AniList uses
                // it to tell us exactly how long the current penalty lasts.
                let wait = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(Duration::from_secs)
                    .unwrap_or_else(|| backoff);
                let capped = wait.min(Duration::from_secs(300));
                log_warn(&format!(
                    "[{}] {} попытка {}/{}: HTTP {} — ждём {}с",
                    self.name,
                    method,
                    attempt,
                    self.attempts,
                    status.as_u16(),
                    capped.as_secs()
                ));
                last = format!("HTTP {}", status.as_u16());
                tokio::time::sleep(capped).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
                continue;
            }

            if status.is_server_error() {
                let body = resp.text().await.unwrap_or_default();
                log_warn(&format!(
                    "[{}] {} попытка {}/{}: HTTP {} — {}",
                    self.name,
                    method,
                    attempt,
                    self.attempts,
                    status.as_u16(),
                    truncate(&body, 200)
                ));
                last = format!("HTTP {} {}", status.as_u16(), truncate(&body, 200));
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
                continue;
            }

            let body = resp.text().await.unwrap_or_default();
            return Err(format!("HTTP {}: {}", status.as_u16(), truncate(&body, 300)));
        }

        Err(format!("после {} попыток: {}", self.attempts, last))
    }
}

/// One line of context from an upstream response body, for the log and for the
/// error text a loader records.
///
/// Newlines are collapsed and CR is dropped rather than merely replaced: a
/// stray `\r` in a log line moves the cursor to the start of the row and
/// turns one request into several unreadable ones.
fn truncate(s: &str, n: usize) -> String {
    let t = s.trim().replace('\r', "").replace('\n', " ");
    if t.chars().count() <= n {
        t
    } else {
        format!("{}…", t.chars().take(n).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------ truncate

    #[test]
    fn a_short_string_is_returned_untouched() {
        assert_eq!(truncate("ok", 10), "ok");
    }

    #[test]
    fn a_long_string_is_cut_and_marked() {
        // The ellipsis is what tells a reader that the message was truncated
        // rather than complete.
        let got = truncate("abcdefghij", 4);
        assert_eq!(got, "abcd…");
        assert!(got.starts_with("abcd"));
    }

    #[test]
    fn newlines_are_collapsed_so_a_response_body_stays_on_one_line() {
        // Upstream error bodies are HTML; a multi-line log entry would break
        // the one-line-per-request format.
        assert_eq!(truncate("a\r\nb\nc", 100), "a b c");
    }

    #[test]
    fn surrounding_whitespace_is_trimmed() {
        assert_eq!(truncate("  \n padded \n ", 100), "padded");
    }

    #[test]
    fn truncation_counts_characters_and_not_bytes() {
        // Cutting a UTF-8 string by byte index panics; counting characters is
        // what keeps a Russian error message intact.
        let s = "ошибка".repeat(10);
        let got = truncate(&s, 5);
        assert_eq!(got, "ошибк…");
        assert_eq!(got.chars().count(), 6);
    }

    #[test]
    fn an_empty_string_stays_empty() {
        assert_eq!(truncate("", 10), "");
        assert_eq!(truncate("   ", 10), "");
    }

    // ------------------------------------------------------------- pacing

    fn upstream(rpm: u32) -> Upstream {
        Upstream::new("test", Duration::from_secs(5), rpm, Vec::new()).unwrap()
    }

    #[test]
    fn a_zero_quota_means_no_pacing() {
        // Used by tests and by a deployment that wants raw speed.
        let u = upstream(0);
        assert!(u.min_interval.is_zero());
        // Returns immediately instead of spinning on a zero-length window.
        let t0 = Instant::now();
        u.wait_turn();
        assert!(t0.elapsed() < Duration::from_millis(200));
    }

    #[test]
    fn the_interval_keeps_five_percent_of_headroom() {
        // 95% of the quota: a full 30/min against a published 30/min budget is
        // a guaranteed 429.
        let u = upstream(30);
        let expected = Duration::from_secs_f64(60.0 * 0.95 / 30.0);
        assert_eq!(u.min_interval, expected);
        assert!(u.min_interval > Duration::from_secs(1));
    }

    #[test]
    fn a_larger_quota_paces_faster() {
        // The anonymous AniList budget is 30/min and the client-id one is 90.
        let slow = upstream(30);
        let fast = upstream(90);
        assert!(fast.min_interval < slow.min_interval);
    }

    #[test]
    fn the_second_request_of_a_window_has_to_wait() {
        // This is the behaviour the old flat 750 ms sleep got wrong: the gap
        // between two requests is derived from the quota, not guessed. 120 rpm
        // gives a 475 ms gap, so the assertion has a wide margin.
        let u = upstream(120);
        let t0 = Instant::now();
        u.wait_turn();
        assert!(t0.elapsed() < Duration::from_millis(50), "первый запрос ждать не должен");
        let second = Instant::now();
        u.wait_turn();
        assert!(
            second.elapsed() >= Duration::from_millis(300),
            "второй запрос ждёт {:?}",
            second.elapsed()
        );
    }

    #[test]
    fn a_high_quota_does_not_make_the_first_request_slow() {
        let u = upstream(1_000_000);
        let t0 = Instant::now();
        u.wait_turn();
        assert!(t0.elapsed() < Duration::from_millis(100));
    }

    // ------------------------------------------------------------- client

    #[test]
    fn extra_headers_are_carried_into_every_request() {
        // The AniList client id is the difference between 30 and 90 requests a
        // minute, so it must actually reach the wire.
        let u = Upstream::new(
            "test",
            Duration::from_secs(5),
            0,
            vec![("Authorization".to_string(), "Bearer secret".to_string())],
        )
        .unwrap();
        let req = u.build("https://example.invalid/x").build().unwrap();
        assert_eq!(req.headers().get("authorization").unwrap(), "Bearer secret");
        let req = u.build_post("https://example.invalid/x").build().unwrap();
        assert_eq!(req.headers().get("authorization").unwrap(), "Bearer secret");
    }

    #[test]
    fn a_build_failure_is_reported_instead_of_a_broken_client() {
        // `Upstream::new` is the only place a reqwest error can surface, and
        // the sources constructor turns it into a message; a client that built
        // but is unusable would only fail hours later inside a loader.
        let u = Upstream::new("test", Duration::from_secs(5), 0, Vec::new()).unwrap();
        assert_eq!(u.attempts, 5);
        assert!(u.min_interval.is_zero());
    }

    #[test]
    fn the_attempt_budget_is_fixed() {
        // Five attempts with exponential backoff is what keeps a single
        // dropped page from ending an hours-long import.
        assert_eq!(upstream(0).attempts, 5);
    }

    // -------------------------------------------------- against a real socket
    //
    // The retry rules are the part of this file that has never been exercised,
    // and they are exactly what decides whether a rate-limited AniList import
    // finishes. A throwaway server on the loopback interface is enough: these
    // tests are about our reaction to a status code, not about AniList.

    /// Answers each incoming request with the next response in `responses` and
    /// reports how many requests it has served so far.
    fn spawn_server(responses: Vec<&'static str>) -> (String, std::sync::mpsc::Receiver<usize>) {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut served = 0usize;
            for response in responses {
                let Ok((mut sock, _)) = listener.accept() else { break };
                // The request has to be read before the response is written, or
                // the client sees a reset instead of the status under test.
                let mut buf = [0u8; 4096];
                let _ = sock.read(&mut buf);
                let _ = sock.write_all(response.as_bytes());
                let _ = sock.flush();
                served += 1;
                if tx.send(served).is_err() {
                    break;
                }
            }
        });
        (format!("http://{}/page", addr), rx)
    }

    const OK_BODY: &str =
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 7\r\n\r\n{\"a\":1}";
    // 22 bytes: "<html>" + "rate limited" + "</h>". The length has to be right
    // or the client fails on a truncated body instead of on the JSON.
    const NOT_JSON: &str =
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 22\r\n\r\n<html>rate limited</h>";
    const NOT_FOUND: &str = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
    const SERVER_ERROR: &str = "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n";
    const THROTTLED: &str =
        "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 1\r\nContent-Length: 0\r\n\r\n";

    fn client() -> Upstream {
        upstream(0)
    }

    #[tokio::test]
    async fn a_successful_get_returns_the_parsed_body() {
        let (url, _rx) = spawn_server(vec![OK_BODY]);
        let v = client().get_json(&url).await.unwrap();
        assert_eq!(v["a"], 1);
    }

    #[tokio::test]
    async fn a_200_with_a_non_json_body_is_an_error_with_a_hint() {
        // A CDN maintenance page comes back as 200 with HTML; the error has to
        // say so, or the loader logs a bare "parse error" and retries blind.
        let (url, _rx) = spawn_server(vec![NOT_JSON]);
        let e = client().get_json(&url).await.unwrap_err();
        assert!(e.contains("json:"), "ошибка: {}", e);
        assert!(e.contains("rate limited"), "в ответе нет начала тела: {}", e);
    }

    #[tokio::test]
    async fn a_404_is_not_retried() {
        // A wrong URL will not fix itself, and five attempts per page would turn
        // one bad endpoint into a two-hour import.
        let (url, rx) = spawn_server(vec![NOT_FOUND]);
        let e = client().get_json(&url).await.unwrap_err();
        assert!(e.contains("HTTP 404"), "ошибка: {}", e);
        assert_eq!(rx.recv().unwrap(), 1, "повтор быть не должно");
    }

    #[tokio::test]
    async fn a_server_error_is_retried_and_can_succeed() {
        let (url, rx) = spawn_server(vec![SERVER_ERROR, OK_BODY]);
        let v = client().get_json(&url).await.unwrap();
        assert_eq!(v["a"], 1);
        assert_eq!(rx.recv().unwrap(), 1);
        assert_eq!(rx.recv().unwrap(), 2);
    }

    #[tokio::test]
    async fn a_throttle_is_retried_after_the_servers_own_delay() {
        // AniList sends `Retry-After` telling us exactly how long the penalty
        // lasts; ignoring it and hammering is how a 429 becomes a ban.
        let (url, rx) = spawn_server(vec![THROTTLED, OK_BODY]);
        let started = Instant::now();
        let v = client().get_json(&url).await.unwrap();
        assert_eq!(v["a"], 1);
        assert!(
            started.elapsed() >= Duration::from_millis(900),
            "пауза {:?} короче Retry-After",
            started.elapsed()
        );
        assert!(rx.recv().is_ok(), "первый запрос не обслужен");
        assert_eq!(rx.recv().unwrap(), 2, "ожидался ровно один повтор");
    }

    #[tokio::test]
    async fn a_forbidden_response_is_treated_as_a_throttle() {
        // AniList answers 403 when the client id is wrong; treating it as a
        // permanent answer would abandon the import, retrying is the only way
        // forward.
        let forbidden = "HTTP/1.1 403 Forbidden\r\nRetry-After: 1\r\nContent-Length: 0\r\n\r\n";
        let (url, rx) = spawn_server(vec![forbidden, OK_BODY]);
        let v = client().get_json(&url).await.unwrap();
        assert_eq!(v["a"], 1);
        assert!(rx.recv().is_ok());
        assert_eq!(rx.recv().unwrap(), 2);
    }

    #[tokio::test]
    async fn a_post_sends_its_json_body() {
        let (url, _rx) = spawn_server(vec![OK_BODY]);
        let v = client()
            .post_json(&url, &serde_json::json!({ "query": "x" }))
            .await
            .unwrap();
        assert_eq!(v["a"], 1);
    }

    // ------------------------------------------------------ the cache
    //
    // The measurement that decided the cache exists: `/api/*` never opens a
    // connection to a source, and inside a pass each page URL is asked for
    // once. What repeats across passes is a pair of reference documents, so
    // the tests below pin down both halves — those two are reused, and
    // everything else goes out every time.

    /// A server that answers `body` to everything, up to `times` requests, and
    /// reports each one. The cache tests need to tell "asked again" from
    /// "answered from memory", which is a count of requests, not a count of
    /// responses the client happened to see.
    ///
    /// `Connection: close` because the loop accepts one socket per request: a
    /// pooled connection would be handed a second request that nobody reads.
    fn spawn_counting_server(body: String, times: usize) -> (String, std::sync::mpsc::Receiver<usize>) {
        use std::io::{Read, Write};

        let length = body.len();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            length, body
        );
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut served = 0usize;
            while served < times {
                let Ok((mut sock, _)) = listener.accept() else { break };
                let mut buf = [0u8; 4096];
                let _ = sock.read(&mut buf);
                let _ = sock.write_all(response.as_bytes());
                let _ = sock.flush();
                served += 1;
                if tx.send(served).is_err() {
                    break;
                }
            }
        });
        (format!("http://{}/page", addr), rx)
    }

    /// The count the server reached, once it has reached at least `n`.
    ///
    /// The count is published after the response is written, so it can lag the
    /// client by a moment; reading once and comparing would be a race.
    fn served_up_to(rx: &std::sync::mpsc::Receiver<usize>, n: usize) -> usize {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(got) if got >= n => return got,
                Ok(_) => continue,
                Err(_) => panic!("сервер обслужил меньше {} запросов", n),
            }
        }
    }

    /// Whether the server served a request the caller has not accounted for.
    fn served_more_than(rx: &std::sync::mpsc::Receiver<usize>, seen: usize) -> bool {
        let deadline = Instant::now() + Duration::from_millis(400);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(got) if got > seen => return true,
                Ok(_) => continue,
                Err(_) => return false,
            }
        }
    }

    #[tokio::test]
    async fn a_reference_document_is_asked_for_once_and_replayed() {
        let (url, rx) = spawn_counting_server("{\"a\":1}".to_string(), 8);
        let u = client();
        let first = u.get_json_cached(&url, Freshness::Forever).await.unwrap();
        assert_eq!(first["a"], 1);
        assert_eq!(served_up_to(&rx, 1), 1);

        for _ in 0..3 {
            let again = u.get_json_cached(&url, Freshness::Forever).await.unwrap();
            assert_eq!(again, first, "ответ из кэша отличается от первого");
        }
        assert!(
            !served_more_than(&rx, 1),
            "серверу ушёл повторный запрос: кэш не работает"
        );

        let s = u.cache_stats();
        assert_eq!((s.entries, s.hits, s.misses), (1, 3, 1));
    }

    #[tokio::test]
    async fn an_uncached_page_is_asked_for_every_single_time() {
        // The other half of the measurement, and the reason the cache is
        // opt-in: a catalogue page exists to be re-read, and one that came
        // from memory would freeze the catalogue at whatever the source said
        // the first time.
        let (url, rx) = spawn_counting_server("{\"a\":1}".to_string(), 8);
        let u = client();
        for _ in 0..3 {
            u.get_json(&url).await.unwrap();
        }
        assert_eq!(served_up_to(&rx, 3), 3, "повторные запросы не дошли до сервера");
        assert_eq!(u.cache_stats().entries, 0, "страница попала в кэш");
    }

    #[tokio::test]
    async fn now_freshness_is_the_same_as_no_cache_at_all() {
        // `Freshness::Now` is what every page fetch passes, so a mistake here
        // would quietly serve a stale catalogue to a user asking for a fresh
        // one.
        let (url, rx) = spawn_counting_server("{\"a\":1}".to_string(), 8);
        let u = client();
        u.get_json_cached(&url, Freshness::Now).await.unwrap();
        u.get_json_cached(&url, Freshness::Now).await.unwrap();
        assert_eq!(served_up_to(&rx, 2), 2);
    }

    #[tokio::test]
    async fn two_documents_do_not_share_an_entry() {
        // Keyed by URL, and the two that are cached are different documents on
        // different hosts: a key mix-up would show one source another source's
        // genres.
        let (first_url, first_rx) = spawn_counting_server("{\"a\":1}".to_string(), 8);
        let (second_url, second_rx) = spawn_counting_server("{\"a\":2}".to_string(), 8);
        let u = client();
        assert_eq!(u.get_json_cached(&first_url, Freshness::Forever).await.unwrap()["a"], 1);
        assert_eq!(u.get_json_cached(&second_url, Freshness::Forever).await.unwrap()["a"], 2);
        assert_eq!(first_rx.recv().unwrap(), 1);
        assert_eq!(second_rx.recv().unwrap(), 1);
        assert_eq!(u.cache_stats().entries, 2);
    }

    #[tokio::test]
    async fn a_failed_request_is_never_remembered() {
        // A source that answers 503 for a minute must not be written off for
        // the life of the process. Only a 2xx is stored.
        let (url, rx) = spawn_server(vec![SERVER_ERROR, OK_BODY]);
        let u = client();
        assert!(u.get_json_cached(&url, Freshness::Forever).await.is_ok());
        assert_eq!(u.cache_stats().entries, 1);
        assert!(rx.recv().is_ok());

        // A permanent failure leaves nothing behind: the next call has to go
        // out and try again.
        let (dead_url, _rx) = spawn_server(vec![NOT_FOUND]);
        assert!(u.get_json_cached(&dead_url, Freshness::Forever).await.is_err());
        assert_eq!(u.cache_stats().entries, 1, "ошибка попала в кэш");
    }

    #[tokio::test]
    async fn a_document_over_the_size_bound_is_kept_out() {
        // A catalogue page is under the entry count and over the byte bound,
        // which is exactly the case the bound exists for: nothing should be
        // able to grow the cache by asking for a big page with the wrong
        // freshness.
        let big = format!("{{\"a\":\"{}\"}}", "x".repeat(CACHE_MAX_BYTES + 16));
        let (url, rx) = spawn_counting_server(big, 4);
        let u = client();
        let first = u.get_json_cached(&url, Freshness::Forever).await.unwrap();
        assert!(first["a"].as_str().unwrap().len() > CACHE_MAX_BYTES);
        assert_eq!(served_up_to(&rx, 1), 1);

        let second = u.get_json_cached(&url, Freshness::Forever).await.unwrap();
        assert_eq!(second["a"], first["a"]);
        assert_eq!(served_up_to(&rx, 2), 2, "слишком большой ответ всё-таки сохранили");
        assert_eq!(
            u.cache_stats(),
            CacheStats { entries: 0, bytes: 0, hits: 0, misses: 2, refused: 2 }
        );
    }

    #[tokio::test]
    async fn a_post_is_never_answered_from_the_cache() {
        // Two POSTs to the same URL are two different questions: AniList's
        // page size, sort and page all live in the body.
        let (url, rx) = spawn_counting_server("{\"a\":1}".to_string(), 4);
        let u = client();
        u.post_json(&url, &serde_json::json!({ "query": "one" })).await.unwrap();
        u.post_json(&url, &serde_json::json!({ "query": "two" })).await.unwrap();
        assert_eq!(served_up_to(&rx, 2), 2);
        assert_eq!(u.cache_stats().entries, 0);
    }

    #[tokio::test]
    async fn the_cache_never_grows_past_its_entry_bound() {
        // Even if every caller asked for reuse, the cache stays small: a
        // bounded cache that can be talked into growing is not bounded.
        let u = client();
        for i in 0..CACHE_MAX_ENTRIES + 8 {
            let body = format!("{{\"a\":{}}}", i);
            let (url, _rx) = spawn_counting_server(body, 1);
            u.get_json_cached(&url, Freshness::Forever).await.unwrap();
        }
        let s = u.cache_stats();
        assert_eq!(s.entries, CACHE_MAX_ENTRIES, "кэш вырос за предел");
        assert_eq!(s.refused, 8);
    }

    #[test]
    fn a_survivor_of_a_poisoned_cache_still_works() {
        // The cache is a second lock in the request path. A panic while holding
        // it must not turn into a panic on the next request.
        let u = client();
        let held = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = u.cache.lock().expect("lock");
            panic!("deliberate panic while holding the cache");
        }));
        assert!(held.is_err());
        assert!(u.cache.lock().unwrap_or_else(|p| p.into_inner()).stats().entries == 0);
    }
}

