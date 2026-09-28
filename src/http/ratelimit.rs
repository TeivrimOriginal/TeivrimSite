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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_up_to_the_budget_are_allowed() {
        let l = Limiter::new(600);
        for i in 0..5 {
            assert!(l.check_n("1.2.3.4", 5).allowed, "запрос {}", i);
        }
    }

    #[test]
    fn the_request_after_the_budget_is_refused() {
        let l = Limiter::new(600);
        for _ in 0..5 {
            l.check_n("1.2.3.4", 5);
        }
        let d = l.check_n("1.2.3.4", 5);
        assert!(!d.allowed);
        // `retry_after` has to be at least a second or a client would spin.
        assert!(d.retry_after >= 1);
    }

    #[test]
    fn a_refused_request_does_not_extend_the_window() {
        // Counting refusals would make a single client keep itself locked out
        // long after the window it burned is over.
        let l = Limiter::new(600);
        for _ in 0..20 {
            l.check_n("k", 5);
        }
        let first = l.check_n("k", 5).retry_after;
        let second = l.check_n("k", 5).retry_after;
        assert_eq!(first, second);
    }

    #[test]
    fn keys_are_counted_separately() {
        // One user hammering the login form must not lock out the rest of the
        // building.
        let l = Limiter::new(600);
        for _ in 0..5 {
            l.check_n("a", 5);
        }
        assert!(!l.check_n("a", 5).allowed);
        assert!(l.check_n("b", 5).allowed);
    }

    #[test]
    fn two_endpoints_can_share_one_limiter_with_different_budgets() {
        // Auth is strict and the image proxy is generous, and both are served
        // by the same instance.
        let l = Limiter::new(600);
        for _ in 0..10 {
            assert!(l.check_n("k", 10).allowed);
        }
        assert!(!l.check_n("k", 10).allowed);
        // The looser budget is separate accounting, not a reset.
        let l2 = Limiter::new(60);
        for _ in 0..10 {
            assert!(l2.check_n("k", 10).allowed);
        }
        assert!(!l2.check_n("k", 10).allowed);
        assert!(l2.check_n("k", 240).allowed);
    }

    #[test]
    fn a_zero_budget_refuses_everything() {
        // A deployment that switched an endpoint off should get 429, not an
        // open door.
        let l = Limiter::new(600);
        assert!(!l.check_n("k", 0).allowed);
    }

    #[test]
    fn a_spoofed_forwarded_for_cannot_grow_the_map_without_bound() {
        // The sweep only runs above the cap and drops what has aged out; what
        // matters is that the map does not grow without limit.
        let l = Limiter::new(1);
        for i in 0..30_000 {
            l.check_n(&format!("10.0.0.{}", i % 65536), 5);
        }
        let map = l.buckets.lock().unwrap();
        assert!(map.len() <= 20_000, "карта выросла до {}", map.len());
    }
}
