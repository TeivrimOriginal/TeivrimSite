//! Fixed-window token bucket per client key, in memory.
//!
//! Scope is deliberately small: this protects password endpoints and the image
//! proxy from being used to hammer the machine or an upstream CDN. It is not a
//! distributed limiter and does not pretend to be — a single process behind one
//! address is the deployment this is built for.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

struct State {
    count: u32,
    window_start: Instant,
}

pub struct Limiter {
    buckets: Mutex<HashMap<String, State>>,
    window_secs: u64,
    /// Cap on tracked keys so a spoofed `X-Forwarded-For` cannot grow the map
    /// without bound.
    max_keys: usize,
}

pub struct Decision {
    pub allowed: bool,
    pub retry_after: u64,
}

impl Limiter {
    /// `window_secs` is the accounting window; the per-endpoint budget is
    /// passed to [`Limiter::check_n`] so one limiter can serve both a strict
    /// auth budget and a looser image budget.
    pub fn new(window_secs: u64) -> Limiter {
        Limiter {
            buckets: Mutex::new(HashMap::new()),
            window_secs,
            max_keys: 20_000,
        }
    }


    /// Consumes one token. Returns whether the request may proceed.
    pub fn check_n(&self, key: &str, limit: u32) -> Decision {
        let now = Instant::now();
        let mut map = self.buckets.lock().unwrap_or_else(|p| p.into_inner());

        if map.len() > self.max_keys {
            // Cheap sweep: drop everything that has already aged out.
            let window = std::time::Duration::from_secs(self.window_secs * 4);
            map.retain(|_, s| now.duration_since(s.window_start) < window);
        }

        let entry = map.entry(key.to_string()).or_insert(State {
            count: 0,
            window_start: now,
        });

        if now.duration_since(entry.window_start) >= std::time::Duration::from_secs(self.window_secs) {
            entry.count = 0;
            entry.window_start = now;
        }

        if entry.count >= limit {
            let elapsed = now.duration_since(entry.window_start).as_secs_f64();
            let retry = ((self.window_secs as f64 - elapsed).max(0.0)).ceil() as u64;
            return Decision {
                allowed: false,
                retry_after: retry.max(1),
            };
        }

        entry.count += 1;
        Decision {
            allowed: true,
            retry_after: 0,
        }
    }

}
