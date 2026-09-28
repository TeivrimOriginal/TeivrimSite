use std::env;
use std::path::PathBuf;

/// Runtime configuration, all overridable through environment variables.
/// `.env` in the working directory is loaded automatically if present.
pub struct Config {
    pub bind_addr: String,
    pub port: u16,
    pub db_path: PathBuf,
    pub frontend_dir: PathBuf,
    /// Allowed CORS origins. `*` allows any origin.
    pub cors_origins: Vec<String>,
    pub pool_size: u32,
    /// Start the background catalogue sync workers on boot.
    pub loaders_on_start: bool,
    /// How often the catalogue is rebuilt from scratch on its own, seconds.
    /// `0` means never; the sync then only happens on boot or by hand.
    ///
    /// A pass costs a few thousand requests across the three sources, so the
    /// default is a full day rather than an hour. There is no "changed since"
    /// filter in any of the three APIs, which is what makes a refresh a full
    /// re-import rather than a delta.
    pub sync_interval_secs: u64,
    /// AniList OAuth client id. Supplying one raises the rate limit from
    /// 30 to 90 requests per minute.
    pub anilist_client_id: Option<String>,
    /// Page size used when pulling catalogues from the upstream APIs.
    pub sync_page_size: u32,
    /// How many titles get characters and staff fetched from Kitsu. Two
    /// requests each, so this is also the cost knob for the free API.
    pub kitsu_enrich_limit: i64,
    /// Max rows a single `/api/list` call may return.
    pub max_per_page: i64,
    /// Upstream request timeout, seconds.
    pub upstream_timeout_secs: u64,
}

fn var(key: &str) -> Option<String> {
    match env::var(key) {
        Ok(v) if !v.trim().is_empty() => Some(v.trim().to_string()),
        _ => None,
    }
}

fn var_or(key: &str, default: &str) -> String {
    var(key).unwrap_or_else(|| default.to_string())
}

fn parse_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    var(key).and_then(|v| v.parse().ok()).unwrap_or(default)
}

impl Config {
    pub fn from_env() -> Config {
        let port: u16 = parse_or("PORT", 8082);
        Config {
            bind_addr: var_or("BIND_ADDR", "127.0.0.1"),
            port,
            db_path: PathBuf::from(var_or("DB_PATH", "data/AnimeData.db")),
            frontend_dir: PathBuf::from(var_or("FRONTEND_DIR", "frontend")),
            cors_origins: var("CORS_ORIGINS")
                .map(|v| {
                    v.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_else(|| vec!["*".to_string()]),
            pool_size: parse_or("DB_POOL_SIZE", 8).clamp(1, 64),
            loaders_on_start: var("LOADERS_ON_START")
                .map(|v| v != "0" && v.to_lowercase() != "false")
                .unwrap_or(true),
            sync_interval_secs: parse_or("SYNC_INTERVAL_SECS", 86_400u64).clamp(0, 30 * 86_400),
            anilist_client_id: var("ANILIST_CLIENT_ID"),
            sync_page_size: parse_or("SYNC_PAGE_SIZE", 50).clamp(1, 50),
            kitsu_enrich_limit: parse_or("KITSU_ENRICH_LIMIT", 2_000i64).clamp(0, 50_000),
            max_per_page: parse_or("MAX_PER_PAGE", 100).clamp(1, 500),
            upstream_timeout_secs: parse_or("UPSTREAM_TIMEOUT_SECS", 45).clamp(5, 300),
        }
    }

    /// `0.0.0.0` means every interface, which is what a container needs.
    pub fn socket_addr(&self) -> String {
        format!("{}:{}", self.bind_addr, self.port)
    }
}
