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
        let anilist_headers: Vec<(String, String)> = cfg
            .anilist_client_id
            .as_ref()
            .map(|id| vec![("Authorization".to_string(), format!("Bearer {}", id))])
            .unwrap_or_default();
        let anilist_rpm = if cfg.anilist_client_id.is_some() { 85 } else { 28 };

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
