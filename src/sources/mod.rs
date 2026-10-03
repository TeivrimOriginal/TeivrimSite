pub mod anilist;
pub mod kitsu;
pub mod shikimori;

use crate::config::Config;
use crate::upstream::Upstream;
use std::time::Duration;

pub struct Sources {
    pub anilist: Upstream,
    pub kitsu: Upstream,
    pub shikimori: Upstream,
}

impl Sources {
    pub fn new(cfg: &Config) -> Result<Sources, String> {
        let timeout = Duration::from_secs(cfg.upstream_timeout_secs);

        // AniList: 30 req/min anonymous, 90 req/min with a client id.
        // Supplying the id is a one-line win, so it is wired through.
        let anilist_headers = anilist_headers(cfg.anilist_client_id.as_ref());
        let anilist_rpm = anilist_quota(cfg.anilist_client_id.is_some());

        Ok(Sources {
            anilist: Upstream::new("anilist", timeout, anilist_rpm, anilist_headers)
                .map_err(|e| format!("reqwest (anilist): {}", e))?,
            // Kitsu is a normal CDN-backed JSON:API; it publishes no quota but
            // throttles bursts, so stay well under a few requests a second.
            kitsu: Upstream::new("kitsu", timeout, 55, Vec::new())
                .map_err(|e| format!("reqwest (kitsu): {}", e))?,
            shikimori: Upstream::new("shikimori", timeout, 50, Vec::new())
                .map_err(|e| format!("reqwest (shikimori): {}", e))?,
        })
    }
}

/// The AniList budget, already discounted for headroom.
///
/// The published limits are 30 requests a minute anonymously and 90 with a
/// client id. A flat figure equal to the limit is a guaranteed 429, so both
/// are given a margin — the same 95% `Upstream` applies to any quota, kept
/// here so the choice is a decision that can be read and tested on its own
/// rather than two bare literals in a constructor.
fn anilist_quota(has_client_id: bool) -> u32 {
    if has_client_id {
        85
    } else {
        28
    }
}

/// The header list AniList is called with.
fn anilist_headers(client_id: Option<&String>) -> Vec<(String, String)> {
    client_id
        .map(|id| vec![("Authorization".to_string(), format!("Bearer {}", id))])
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with_client_id(id: Option<&str>) -> Config {
        Config {
            anilist_client_id: id.map(|s| s.to_string()),
            ..Config::from_env()
        }
    }

    #[test]
    fn a_client_id_raises_the_anilist_budget() {
        // The single biggest speed-up available: 30 to 90 requests a minute on
        // the longest stage of the import.
        assert!(anilist_quota(true) > anilist_quota(false));
    }

    #[test]
    fn both_budgets_stay_under_what_the_api_publishes() {
        // Asking for exactly the published limit is a guaranteed 429, and the
        // retry storm that follows costs more than the requests saved.
        assert!(anilist_quota(false) < 30, "анонимный лимит превышен");
        assert!(anilist_quota(true) < 90, "лимит с client id превышен");
    }

    #[test]
    fn the_client_id_is_sent_as_a_bearer_token() {
        // A client id sent any other way is ignored by AniList, and the import
        // silently stays on the anonymous budget for the whole run.
        assert_eq!(
            anilist_headers(Some(&"abc123".to_string())),
            vec![("Authorization".to_string(), "Bearer abc123".to_string())]
        );
        assert!(anilist_headers(None).is_empty());
    }

    #[test]
    fn all_three_sources_are_built_from_the_configuration() {
        // A source that needs longer than the global timeout is a source that
        // always fails, and the difference has to be visible at construction
        // rather than three hours into an import.
        let mut cfg = cfg_with_client_id(None);
        cfg.upstream_timeout_secs = 7;
        assert!(Sources::new(&cfg).is_ok());
    }

    #[test]
    fn each_source_gets_its_own_client() {
        // One shared client would share the rate-limit window too, so the
        // three sources would throttle each other and AniList's budget would
        // gate Kitsu and Shikimori as well.
        let s = Sources::new(&cfg_with_client_id(Some("abc"))).expect("sources");
        let anilist = format!("{:p}", &s.anilist);
        let kitsu = format!("{:p}", &s.kitsu);
        let shikimori = format!("{:p}", &s.shikimori);
        assert_ne!(anilist, kitsu);
        assert_ne!(kitsu, shikimori);
        assert_ne!(anilist, shikimori);
    }
}
