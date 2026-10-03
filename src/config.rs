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

impl Config {
    pub fn from_env() -> Config {
        Config::from_lookup(|k| env::var(k).ok())
    }

    /// The same thing, reading through a lookup the caller supplies.
    ///
    /// Configuration is the one thing in this codebase that is genuinely
    /// global, and the clamps below are exactly the code that is never
    /// exercised in production because an operator does not set
    /// `DB_POOL_SIZE=0` on purpose. Driving it from a map makes the whole
    /// decision tree testable without a process-wide environment, which the
    /// parallel test harness would not allow anyway.
    ///
    /// The lookup is a source of strings and nothing more: what counts as
    /// "set" is decided here, once, rather than by each caller.
    fn from_lookup(mut get: impl FnMut(&str) -> Option<String>) -> Config {
        let mut var = |k: &str| {
            get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let port: u16 = num(var("PORT"), 8082);
        Config {
            bind_addr: or(var("BIND_ADDR"), "127.0.0.1"),
            port,
            db_path: PathBuf::from(or(var("DB_PATH"), "data/AnimeData.db")),
            frontend_dir: PathBuf::from(or(var("FRONTEND_DIR"), "frontend")),
            cors_origins: origins(var("CORS_ORIGINS")),
            pool_size: num(var("DB_POOL_SIZE"), 8).clamp(1, 64),
            loaders_on_start: flag(var("LOADERS_ON_START"), true),
            sync_interval_secs: num(var("SYNC_INTERVAL_SECS"), 86_400).clamp(0, 30 * 86_400),
            anilist_client_id: var("ANILIST_CLIENT_ID"),
            sync_page_size: num(var("SYNC_PAGE_SIZE"), 50).clamp(1, 50),
            kitsu_enrich_limit: num(var("KITSU_ENRICH_LIMIT"), 2_000).clamp(0, 50_000),
            max_per_page: num(var("MAX_PER_PAGE"), 100).clamp(1, 500),
            upstream_timeout_secs: num(var("UPSTREAM_TIMEOUT_SECS"), 45).clamp(5, 300),
        }
    }

    /// `0.0.0.0` means every interface, which is what a container needs.
    pub fn socket_addr(&self) -> String {
        format!("{}:{}", self.bind_addr, self.port)
    }
}

/// The CORS allow-list, or `*` when there is nothing to allow.
///
/// A list that trims down to nothing — `CORS_ORIGINS=,` in a compose file, or
/// a variable that expanded to nothing — reads as "configured" and would
/// refuse every browser while looking deliberate. An allow-list of zero entries
/// is never what anyone meant.
fn origins(value: Option<String>) -> Vec<String> {
    let list: Vec<String> = value
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if list.is_empty() {
        vec!["*".to_string()]
    } else {
        list
    }
}

fn or(value: Option<String>, default: &str) -> String {
    value.unwrap_or_else(|| default.to_string())
}

/// A number, falling back when the value is missing or not a number at all.
fn num<T: std::str::FromStr>(value: Option<String>, default: T) -> T {
    value.and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// A flag read the way a deployment expects it: `0` and `false` are off,
/// anything else that is set is on.
fn flag(value: Option<String>, default: bool) -> bool {
    match value {
        Some(v) => v != "0" && !v.eq_ignore_ascii_case("false"),
        None => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> Config {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_lookup(|k| map.get(k).cloned())
    }

    #[test]
    fn an_empty_environment_gets_the_documented_defaults() {
        let c = cfg(&[]);
        assert_eq!(c.bind_addr, "127.0.0.1");
        assert_eq!(c.port, 8082);
        assert_eq!(c.db_path, PathBuf::from("data/AnimeData.db"));
        assert_eq!(c.frontend_dir, PathBuf::from("frontend"));
        assert_eq!(c.cors_origins, vec!["*"]);
        assert_eq!(c.pool_size, 8);
        assert!(
            c.loaders_on_start,
            "загрузчики должны стартовать по умолчанию"
        );
        assert_eq!(c.sync_interval_secs, 86_400);
        assert_eq!(c.sync_page_size, 50);
        assert_eq!(c.kitsu_enrich_limit, 2_000);
        assert_eq!(c.max_per_page, 100);
        assert_eq!(c.upstream_timeout_secs, 45);
        assert_eq!(c.anilist_client_id, None);
    }

    #[test]
    fn a_blank_value_is_the_same_as_not_setting_it() {
        // `DB_PATH=` in a docker-compose file is a common way to end up with an
        // empty path, and it must not become `PathBuf::from("")`.
        let c = cfg(&[("DB_PATH", "   "), ("BIND_ADDR", "")]);
        assert_eq!(c.db_path, PathBuf::from("data/AnimeData.db"));
        assert_eq!(c.bind_addr, "127.0.0.1");
    }

    #[test]
    fn a_value_is_trimmed() {
        let c = cfg(&[("ANILIST_CLIENT_ID", "  abc123  ")]);
        assert_eq!(c.anilist_client_id.as_deref(), Some("abc123"));
    }

    #[test]
    fn cors_origins_are_split_trimmed_and_compacted() {
        let c = cfg(&[("CORS_ORIGINS", " https://a.example , ,https://b.example ")]);
        assert_eq!(
            c.cors_origins,
            vec!["https://a.example", "https://b.example"]
        );
    }

    #[test]
    fn an_empty_cors_list_falls_back_to_any_origin() {
        // `CORS_ORIGINS=,` trims to nothing, and an allow-list of nothing
        // would refuse every browser instead of allowing them.
        let c = cfg(&[("CORS_ORIGINS", " , ")]);
        assert_eq!(c.cors_origins, vec!["*"]);
    }

    #[test]
    fn the_pool_can_never_be_empty_or_absurd() {
        assert_eq!(cfg(&[("DB_POOL_SIZE", "0")]).pool_size, 1);
        assert_eq!(cfg(&[("DB_POOL_SIZE", "9999")]).pool_size, 64);
    }

    #[test]
    fn the_page_size_never_exceeds_what_the_sources_accept() {
        // AniList's maximum is 50; asking for more returns HTTP 400 and a
        // loader that then retries the same bad request five times.
        assert_eq!(cfg(&[("SYNC_PAGE_SIZE", "0")]).sync_page_size, 1);
        assert_eq!(cfg(&[("SYNC_PAGE_SIZE", "500")]).sync_page_size, 50);
    }

    #[test]
    fn the_upstream_timeout_has_a_floor_and_a_ceiling() {
        // Below five seconds a slow source is a failed source; above five
        // minutes a dead one holds a loader for the whole backoff chain.
        assert_eq!(
            cfg(&[("UPSTREAM_TIMEOUT_SECS", "1")]).upstream_timeout_secs,
            5
        );
        assert_eq!(
            cfg(&[("UPSTREAM_TIMEOUT_SECS", "9999")]).upstream_timeout_secs,
            300
        );
    }

    #[test]
    fn zero_stays_zero_where_zero_means_off() {
        // Two different off switches, and they must not be confused: zero
        // disables the timer and the enrichment stage outright, while a
        // zero page size or a zero page cap would only break things.
        assert_eq!(cfg(&[("SYNC_INTERVAL_SECS", "0")]).sync_interval_secs, 0);
        assert_eq!(cfg(&[("KITSU_ENRICH_LIMIT", "0")]).kitsu_enrich_limit, 0);
        assert_eq!(cfg(&[("KITSU_ENRICH_LIMIT", "-5")]).kitsu_enrich_limit, 0);
        assert_eq!(cfg(&[("MAX_PER_PAGE", "0")]).max_per_page, 1);
    }

    #[test]
    fn a_negative_refresh_interval_is_off_rather_than_a_panic() {
        // An unsigned field with a signed-looking value: the parse has to fail
        // into the default instead of wrapping.
        assert_eq!(
            cfg(&[("SYNC_INTERVAL_SECS", "-1")]).sync_interval_secs,
            86_400
        );
    }

    #[test]
    fn a_refresh_interval_longer_than_a_month_is_clamped() {
        assert_eq!(
            cfg(&[("SYNC_INTERVAL_SECS", "99999999")]).sync_interval_secs,
            30 * 86_400
        );
    }

    #[test]
    fn garbage_in_a_numeric_variable_falls_back_to_the_default() {
        let c = cfg(&[
            ("PORT", "http"),
            ("DB_POOL_SIZE", "eight"),
            ("MAX_PER_PAGE", ""),
        ]);
        assert_eq!(c.port, 8082);
        assert_eq!(c.pool_size, 8);
        assert_eq!(c.max_per_page, 100);
    }

    #[test]
    fn the_loaders_switch_reads_the_spellings_a_deployment_uses() {
        for off in ["0", "false", "FALSE", "False"] {
            assert!(
                !cfg(&[("LOADERS_ON_START", off)]).loaders_on_start,
                "{}",
                off
            );
        }
        for on in ["1", "true", "yes", "anything"] {
            assert!(cfg(&[("LOADERS_ON_START", on)]).loaders_on_start, "{}", on);
        }
    }

    #[test]
    fn the_listen_address_is_built_from_the_two_halves() {
        assert_eq!(cfg(&[]).socket_addr(), "127.0.0.1:8082");
        assert_eq!(
            cfg(&[("BIND_ADDR", "0.0.0.0"), ("PORT", "80")]).socket_addr(),
            "0.0.0.0:80"
        );
    }

    #[test]
    fn the_real_environment_is_readable_at_all() {
        // The lookup the tests use is a stand-in, so this covers the seam
        // itself: `from_env` must still work and must not be left unparsed.
        let c = Config::from_env();
        assert!(c.port > 0);
        assert!(!c.db_path.as_os_str().is_empty());
    }
}
