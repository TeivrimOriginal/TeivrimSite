use crate::error::log_warn;
use serde_json::Value;
use std::sync::Mutex;
use std::time::{Duration, Instant};

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
}

impl Upstream {
    pub fn new(
        name: &'static str,
        base_timeout: Duration,
        requests_per_minute: u32,
        extra_headers: Vec<(String, String)>,
    ) -> reqwest::Result<Upstream> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("anime-db/2.0 (+https://github.com/TeivrimOriginal/TeivrimSite)"))
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
    /// exponential backoff.
    pub async fn get_json(&self, url: &str) -> Result<Value, String> {
        self.request_with(|rb| async move { rb.send().await }, url, "GET").await
    }

    pub async fn post_json(&self, url: &str, body: &Value) -> Result<Value, String> {
        self.request_with(
            |rb| async move { rb.json(body).send().await },
            url,
            "POST",
        )
        .await
    }

    async fn request_with<F, Fut>(&self, send: F, url: &str, method: &str) -> Result<Value, String>
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
                return serde_json::from_str(&text)
                    .map_err(|e| format!("json: {} (начало ответа: {})", e, truncate(&text, 200)));
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

fn truncate(s: &str, n: usize) -> String {
    let t = s.trim().replace('\n', " ");
    if t.chars().count() <= n {
        t
    } else {
        format!("{}…", t.chars().take(n).collect::<String>())
    }
}

