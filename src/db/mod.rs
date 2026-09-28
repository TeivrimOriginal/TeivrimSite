pub mod pool;

use crate::error::{log_error, log_info, log_warn, now_ts};
use pool::{Pool, PooledConn};
use rusqlite::Connection;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub use pool::PoolError;

pub struct Db {
    pool: Arc<Pool>,
    /// False when the bundled SQLite was built without FTS5. Search then falls
    /// back to LIKE scans instead of hard-failing at boot.
    fts: AtomicBool,
}

/// Handle handed to request handlers. Cheap to clone (one `Arc` bump).
#[derive(Clone)]
pub struct Handle(Arc<Db>);

impl Handle {
    /// Blocking call. Must be used inside `web::block`.
    pub fn conn(&self) -> Result<PooledConn<'_>, PoolError> {
        self.0.pool.get()
    }

    /// Convenience for read-only work that maps the SQLite error into
    /// [`PoolError`], so the success type is the closure's value.
    pub fn with<T>(
        &self,
        f: impl FnOnce(&Connection) -> Result<T, rusqlite::Error>,
    ) -> Result<T, PoolError> {
        let conn = self.conn()?;
        f(&conn).map_err(|e| {
            log_error(&format!("sqlite: {}", e));
            PoolError::Init(e.to_string())
        })
    }

    pub fn fts_enabled(&self) -> bool {
        self.0.fts.load(Ordering::Relaxed)
    }
}

impl Db {
    /// Opens the database, applies the schema, and returns the shared handle.
    ///
    /// Named `open` rather than `new` because the result is a `Handle` wrapping
    /// an `Arc`, not the `Db` itself: what the rest of the program holds is the
    /// cheap, shareable thing.
    pub fn open(db_path: &Path, pool_size: u32) -> Result<Handle, String> {
        if let Some(parent) = db_path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot create {}: {}", parent.display(), e))?;
            }
        }

        handle_legacy_db(db_path);

        let pool = Pool::new(db_path, pool_size as usize).map_err(|e| e.to_string())?;
        let fts = apply_schema(&pool).map_err(|e| e.to_string())?;

        let handle = Handle(Arc::new(Db {
            pool: Arc::new(pool),
            fts: AtomicBool::new(fts),
        }));

        let stats = handle
            .with(|c| Ok((count(c, "anime")?, count(c, "genres")?, count(c, "users")?)))
            .unwrap_or((0, 0, 0));
        log_info(&format!(
            "[db] {} | аниме: {} | жанры: {} | пользователи: {} | FTS5: {}",
            db_path.display(),
            stats.0,
            stats.1,
            stats.2,
            if fts { "да" } else { "нет (LIKE-режим)" }
        ));

        Ok(handle)
    }
}

fn count(conn: &Connection, table: &str) -> rusqlite::Result<i64> {
    conn.query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |r| r.get(0))
}

/// The pre-2.0 schema used `anilist_id INTEGER PRIMARY KEY` and stored
/// source-only rows under a negative id borrowed from the same number space, so
/// two different sources could collide on one row. That data is a pure cache of
/// public APIs and is cheap to rebuild, so an old file is moved aside rather
/// than migrated.
fn handle_legacy_db(db_path: &Path) {
    if !Path::new(db_path).exists() {
        return;
    }

    let Ok(conn) = Connection::open(db_path) else {
        return;
    };
    let has_table: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='anime'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|n| n > 0)
        .unwrap_or(false);
    if !has_table {
        return;
    }

    // v2 has a `uid` column; v1 keyed the table on `anilist_id`.
    let column_count = |name: &str| -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('anime') WHERE name = ?1",
            [name],
            |r| r.get(0),
        )
        .unwrap_or(0)
    };
    let is_legacy = column_count("anilist_id") > 0 && column_count("uid") == 0;
    drop(conn);

    if !is_legacy {
        return;
    }

    let stamp = now_ts();
    let target = format!("{}.legacy-{}", db_path.display(), stamp);
    log_warn(&format!(
        "[db] обнаружена старая схема БД, переношу её в {} (данные публичных API восстанавливаются автоматически)",
        target
    ));
    // SQLite keeps -wal and -shm siblings; move them too or the rename leaves a
    // corrupt database behind.
    for suffix in ["-wal", "-shm"] {
        let src = format!("{}{}", db_path.display(), suffix);
        if Path::new(&src).exists() {
            let _ = std::fs::rename(&src, format!("{}{}", target, suffix));
        }
    }
    if std::fs::rename(db_path, &target).is_err() {
        log_error("[db] не удалось переместить старую БД, удаляю её");
        let _ = std::fs::remove_file(db_path);
    }
}

const SCHEMA_VERSION: i64 = 2;

fn apply_schema(pool: &Pool) -> Result<bool, rusqlite::Error> {
    let conn = pool
        .get()
        .map_err(|e| rusqlite::Error::InvalidParameterName(e.to_string()))?;
    apply_schema_on(&conn)
}

/// The schema, applied to an arbitrary connection.
///
/// Split out from [`apply_schema`] so tests can build a real catalogue in an
/// in-memory database and exercise the actual SQL rather than a hand-written
/// approximation of it: a fake schema in a test proves nothing about the real
/// one.
pub(crate) fn apply_schema_on(conn: &Connection) -> Result<bool, rusqlite::Error> {
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;

        ------------------------------------------------------------------
        -- Catalogue
        ------------------------------------------------------------------
        -- `uid` is the primary key because ids from different sources live in
        -- different number spaces. v1 used an INTEGER primary key and encoded
        -- "not on AniList" as a negative id, which let a MAL entry and a
        -- Shikimori entry with the same number overwrite each other.
        -- uid format: "al:16498" (AniList) | "ks:12" (Kitsu) | "sh:34" (Shikimori)
        CREATE TABLE IF NOT EXISTS anime (
            uid                 TEXT PRIMARY KEY,
            anilist_id          INTEGER UNIQUE,
            kitsu_id            INTEGER UNIQUE,
            shikimori_id        INTEGER UNIQUE,
            mal_id              INTEGER,

            title_romaji        TEXT,
            title_english       TEXT,
            title_native        TEXT,
            title_russian       TEXT,
            -- Lowercased/trimmed title_romaji. Shikimori exposes only names, no
            -- external ids, so the join between the two sources is an exact
            -- match on this column rather than a fuzzy comparison at runtime.
            title_key           TEXT,
            -- Every other name we know about, JSON array of strings.
            alt_titles          TEXT,

            format              TEXT,
            status              TEXT,
            description         TEXT,
            description_ru      TEXT,
            duration            INTEGER,
            episodes            INTEGER,
            chapters            INTEGER,
            volumes             INTEGER,
            country_of_origin   TEXT,
            is_adult            INTEGER NOT NULL DEFAULT 0,
            is_licensed         INTEGER,

            season              TEXT,
            season_year         INTEGER,
            start_date          TEXT,
            end_date            TEXT,
            start_year          INTEGER,
            start_month         INTEGER,
            start_day           INTEGER,
            end_year            INTEGER,
            end_month           INTEGER,
            end_day             INTEGER,

            -- Merged rating on a single 0..100 scale so the UI never has to
            -- reconcile AniList's 0..100 with a 0..10 source.
            score               INTEGER,
            score_source        TEXT,
            mean_score          INTEGER,
            popularity          INTEGER,
            favourites          INTEGER,
            trending            INTEGER,
            rating_count        INTEGER,

            cover_small         TEXT,
            cover_medium        TEXT,
            cover_large         TEXT,
            cover_color         TEXT,
            banner              TEXT,

            trailer_id          TEXT,
            trailer_site        TEXT,
            trailer_thumbnail   TEXT,

            genres_json         TEXT,
            tags_json           TEXT,
            studios_json        TEXT,
            producers_json      TEXT,
            licensors_json      TEXT,
            classifications_json TEXT,
            relations_json      TEXT,
            external_links_json TEXT,
            streaming_json      TEXT,
            recommendations_json TEXT,
            characters_json     TEXT,
            staff_json          TEXT,

            created_at          INTEGER,
            updated_at          INTEGER,
            anilist_synced_at   INTEGER,
            kitsu_synced_at     INTEGER,
            shikimori_synced_at INTEGER,
            genres_matched_at   INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_anime_mal         ON anime(mal_id);
        CREATE INDEX IF NOT EXISTS idx_anime_romaji      ON anime(title_romaji);
        CREATE INDEX IF NOT EXISTS idx_anime_titlekey    ON anime(title_key);
        CREATE INDEX IF NOT EXISTS idx_anime_russian     ON anime(title_russian);
        CREATE INDEX IF NOT EXISTS idx_anime_year        ON anime(start_year);
        CREATE INDEX IF NOT EXISTS idx_anime_score       ON anime(score DESC, popularity DESC);
        CREATE INDEX IF NOT EXISTS idx_anime_popularity  ON anime(popularity DESC, anilist_id ASC);
        CREATE INDEX IF NOT EXISTS idx_anime_updated     ON anime(updated_at DESC);
        CREATE INDEX IF NOT EXISTS idx_anime_format      ON anime(format);
        CREATE INDEX IF NOT EXISTS idx_anime_status      ON anime(status);
        CREATE INDEX IF NOT EXISTS idx_anime_season      ON anime(season, season_year);
        CREATE INDEX IF NOT EXISTS idx_anime_country     ON anime(country_of_origin);
        CREATE INDEX IF NOT EXISTS idx_anime_adult       ON anime(is_adult, score DESC);
        CREATE INDEX IF NOT EXISTS idx_anime_favourites  ON anime(favourites DESC);
        CREATE INDEX IF NOT EXISTS idx_anime_trending    ON anime(trending DESC);
        CREATE INDEX IF NOT EXISTS idx_anime_episodes    ON anime(episodes DESC);
        -- Partial index: the "has a Russian title" filter is common and the
        -- predicate is selective, so this stays small.
        CREATE INDEX IF NOT EXISTS idx_anime_ru_present  ON anime(uid)
            WHERE title_russian IS NOT NULL AND title_russian <> '';

        ------------------------------------------------------------------
        -- Genres
        ------------------------------------------------------------------
        -- `slug` is the lowercased name and carries the uniqueness. v1 put
        -- UNIQUE on name_en and then looked rows up with LOWER(name_en), which
        -- both wasted the index and could still violate the constraint.
        CREATE TABLE IF NOT EXISTS genres (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            slug        TEXT NOT NULL UNIQUE,
            name_en     TEXT NOT NULL,
            name_ru     TEXT,
            category    TEXT,
            created_at  INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_genres_category ON genres(category, name_en);
        CREATE INDEX IF NOT EXISTS idx_genres_ru       ON genres(name_ru);

        CREATE TABLE IF NOT EXISTS anime_genres (
            uid        TEXT NOT NULL,
            genre_id   INTEGER NOT NULL,
            source     TEXT,
            PRIMARY KEY (uid, genre_id)
        );
        CREATE INDEX IF NOT EXISTS idx_ag_genre ON anime_genres(genre_id);
        CREATE INDEX IF NOT EXISTS idx_ag_uid   ON anime_genres(uid);

        ------------------------------------------------------------------
        -- Accounts
        ------------------------------------------------------------------
        CREATE TABLE IF NOT EXISTS users (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            username      TEXT NOT NULL,
            username_key  TEXT NOT NULL UNIQUE,
            email         TEXT,
            email_key     TEXT UNIQUE,
            password_hash TEXT NOT NULL,
            created_at    INTEGER NOT NULL,
            last_login_at INTEGER
        );

        -- Only the SHA-256 of the bearer token is stored, so a database leak
        -- does not hand out working sessions.
        CREATE TABLE IF NOT EXISTS auth_tokens (
            token_hash   TEXT PRIMARY KEY,
            user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            created_at   INTEGER NOT NULL,
            expires_at   INTEGER NOT NULL,
            last_used_at INTEGER,
            user_agent   TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_tokens_user   ON auth_tokens(user_id);
        CREATE INDEX IF NOT EXISTS idx_tokens_expiry ON auth_tokens(expires_at);

        ------------------------------------------------------------------
        -- Watchlist
        ------------------------------------------------------------------
        CREATE TABLE IF NOT EXISTS favorites (
            user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            uid        TEXT NOT NULL,
            status     TEXT NOT NULL DEFAULT 'planned',
            is_favorite INTEGER NOT NULL DEFAULT 0,
            score      INTEGER,
            progress   INTEGER,
            episodes   INTEGER,
            notes      TEXT,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            PRIMARY KEY (user_id, uid)
        );
        CREATE INDEX IF NOT EXISTS idx_fav_user    ON favorites(user_id, status, updated_at DESC);
        CREATE INDEX IF NOT EXISTS idx_fav_fav     ON favorites(user_id, is_favorite, updated_at DESC);

        ------------------------------------------------------------------
        -- Sync bookkeeping
        ------------------------------------------------------------------
        CREATE TABLE IF NOT EXISTS sync_state (
            source      TEXT NOT NULL,
            task        TEXT NOT NULL,
            last_page   INTEGER NOT NULL DEFAULT 0,
            total_saved INTEGER NOT NULL DEFAULT 0,
            finished    INTEGER NOT NULL DEFAULT 0,
            last_error  TEXT,
            started_at  INTEGER,
            ended_at    INTEGER,
            updated_at  INTEGER NOT NULL,
            PRIMARY KEY (source, task)
        );
        "#,
    )?;

    // FTS5 is optional at build time, so the index is created in a second step
    // and the result is remembered.
    let fts = create_fts(conn).unwrap_or_else(|e| {
        log_warn(&format!(
            "[db] FTS5 недоступен ({}), поиск переключён на LIKE-сканы",
            e
        ));
        false
    });

    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;

    if fts {
        let indexed: i64 = conn
            .query_row("SELECT COUNT(*) FROM anime_fts", [], |r| r.get(0))
            .unwrap_or(0);
        let total = count(conn, "anime")?;
        if indexed != total {
            log_info(&format!(
                "[db] пересобираю поисковый индекс ({} из {} записей)",
                indexed, total
            ));
            if let Err(e) = rebuild_fts(conn) {
                log_error(&format!("[db] индекс поиска не пересобран: {}", e));
            }
        }
    }

    Ok(fts)
}

fn create_fts(conn: &Connection) -> Result<bool, rusqlite::Error> {
    // A plain (non-contentless) table keeps `rebuild` available and keeps the
    // rowid mapping trivial. `unicode61` folds case for every script, which is
    // what makes Cyrillic search work — plain LIKE only folds ASCII.
    match conn.execute_batch(
        r#"
        CREATE VIRTUAL TABLE IF NOT EXISTS anime_fts USING fts5(
            uid UNINDEXED,
            title_romaji,
            title_english,
            title_native,
            title_russian,
            alt_titles,
            tokenize = "unicode61 remove_diacritics 2"
        );
        "#,
    ) {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Rebuilds the whole search index from `anime`. Called after a source sync
/// finishes rather than per row: triggers would multiply the cost of the
/// upsert-heavy loaders for no benefit.
pub fn rebuild_fts(conn: &Connection) -> Result<usize, rusqlite::Error> {
    conn.execute("DELETE FROM anime_fts", [])?;
    let n = conn.execute(
        "INSERT INTO anime_fts (uid, title_romaji, title_english, title_native, title_russian, alt_titles)
         SELECT uid, COALESCE(title_romaji,''), COALESCE(title_english,''),
                COALESCE(title_native,''), COALESCE(title_russian,''), COALESCE(alt_titles,'')
         FROM anime",
        [],
    )?;
    Ok(n)
}

// ==================================================================
// sync_state
// ==================================================================

pub struct Checkpoint {
    pub last_page: i64,
    pub total_saved: i64,
    pub finished: bool,
}

pub fn get_checkpoint(conn: &Connection, source: &str, task: &str) -> Checkpoint {
    conn.query_row(
        "SELECT last_page, total_saved, finished FROM sync_state WHERE source = ?1 AND task = ?2",
        rusqlite::params![source, task],
        |r| {
            Ok(Checkpoint {
                last_page: r.get(0)?,
                total_saved: r.get(1)?,
                finished: r.get::<_, i64>(2)? != 0,
            })
        },
    )
    .unwrap_or(Checkpoint {
        last_page: 0,
        total_saved: 0,
        finished: false,
    })
}

pub fn save_checkpoint(
    conn: &Connection,
    source: &str,
    task: &str,
    last_page: i64,
    total_saved: i64,
    finished: bool,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO sync_state (source, task, last_page, total_saved, finished, updated_at, ended_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, CASE WHEN ?5 = 1 THEN ?6 ELSE NULL END)
         ON CONFLICT(source, task) DO UPDATE SET
            last_page   = excluded.last_page,
            total_saved = excluded.total_saved,
            finished    = excluded.finished,
            last_error  = NULL,
            updated_at  = excluded.updated_at,
            ended_at    = excluded.ended_at",
        rusqlite::params![source, task, last_page, total_saved, if finished { 1 } else { 0 }, now_ts()],
    )?;
    Ok(())
}

pub fn mark_started(conn: &Connection, source: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO sync_state (source, task, started_at, updated_at)
         VALUES (?1, 'run', ?2, ?2)
         ON CONFLICT(source, task) DO UPDATE SET
            started_at = excluded.started_at,
            updated_at = excluded.updated_at",
        rusqlite::params![source, now_ts()],
    )?;
    Ok(())
}

pub fn mark_error(conn: &Connection, source: &str, task: &str, err: &str) {
    let short: String = err.chars().take(400).collect();
    if let Err(e) = conn.execute(
        "INSERT INTO sync_state (source, task, last_error, updated_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(source, task) DO UPDATE SET
            last_error = excluded.last_error,
            updated_at = excluded.updated_at",
        rusqlite::params![source, task, short, now_ts()],
    ) {
        log_error(&format!("[sync] не удалось записать ошибку: {}", e));
    }
}

// ==================================================================
// Tests
// ==================================================================

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use rusqlite::Connection;

    /// A fully migrated in-memory catalogue.
    ///
    /// Every test in the crate that needs the database goes through here, so
    /// the SQL under test is the same SQL production runs — a fixture written
    /// by hand would drift from the migrations on the first column change and
    /// keep passing while the real thing broke.
    pub(crate) fn conn() -> Connection {
        let c = Connection::open_in_memory().expect("in-memory sqlite");
        apply_schema_on(&c).expect("schema");
        c
    }

    /// Inserts one minimal catalogue row. Returns its uid.
    pub(crate) fn insert_anime(conn: &Connection, uid: &str, title_romaji: Option<&str>) {
        conn.execute(
            "INSERT INTO anime (uid, title_romaji, title_key, is_adult, created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, 1, 1)",
            rusqlite::params![
                uid,
                title_romaji,
                title_romaji.map(crate::loader::title_key)
            ],
        )
        .expect("insert anime");
    }

    /// A file-backed catalogue behind a real [`Handle`], for the code that only
    /// accepts a handle: the auth endpoints, the watchlist and the handlers.
    ///
    /// SQLite gives every `:memory:` connection its own private database, so a
    /// pool of them would see `no such table: users`. A file per test is the
    /// only way to get the same object a running server holds.
    pub(crate) struct TestDb {
        pub handle: Handle,
        path: std::path::PathBuf,
    }

    impl TestDb {
        pub(crate) fn conn(&self) -> pool::PooledConn<'_> {
            self.handle.conn().expect("connection from the pool")
        }
    }

    impl Drop for TestDb {
        fn drop(&mut self) {
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{}", self.path.display(), suffix));
            }
        }
    }

    pub(crate) fn test_db() -> TestDb {
        use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
        let path = std::env::temp_dir().join(format!("anime-db-test-{}-{}.db", std::process::id(), n));
        TestDb {
            handle: Db::open(&path, 2).unwrap_or_else(|e| panic!("Db::open: {}", e)),
            path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{conn, insert_anime};
    use super::*;

    // ---------------------------------------------------------- checkpoints

    #[test]
    fn checkpoint_defaults_to_the_start_for_an_unknown_task() {
        // Loaders read this before every pass; a missing row must read as
        // "page 0, nothing saved, not finished" rather than error out.
        let c = conn();
        let cp = get_checkpoint(&c, "anilist", "sort:POPULARITY_DESC");
        assert_eq!(cp.last_page, 0);
        assert_eq!(cp.total_saved, 0);
        assert!(!cp.finished);
    }

    #[test]
    fn checkpoint_round_trips_through_save() {
        let c = conn();
        save_checkpoint(&c, "anilist", "sort:ID", 7, 350, false).unwrap();
        let cp = get_checkpoint(&c, "anilist", "sort:ID");
        assert_eq!((cp.last_page, cp.total_saved, cp.finished), (7, 350, false));
    }

    #[test]
    fn checkpoint_is_upserted_per_task_not_per_source() {
        let c = conn();
        save_checkpoint(&c, "anilist", "sort:ID", 3, 150, true).unwrap();
        save_checkpoint(&c, "anilist", "sort:SCORE_DESC", 1, 50, false).unwrap();
        assert!(get_checkpoint(&c, "anilist", "sort:ID").finished);
        assert_eq!(get_checkpoint(&c, "anilist", "sort:SCORE_DESC").last_page, 1);
        assert!(!get_checkpoint(&c, "anilist", "sort:TRENDING_DESC").finished);
    }

    #[test]
    fn saving_a_checkpoint_clears_a_previous_error() {
        // A task that is progressing again must not keep showing the error from
        // the attempt before it, or the progress page stays red forever.
        let c = conn();
        mark_error(&c, "kitsu", "sort:ID", "HTTP 500");
        assert_eq!(
            c.query_row(
                "SELECT last_error FROM sync_state WHERE source = 'kitsu' AND task = 'sort:ID'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "HTTP 500"
        );
        save_checkpoint(&c, "kitsu", "sort:ID", 2, 20, false).unwrap();
        let err: Option<String> = c
            .query_row(
                "SELECT last_error FROM sync_state WHERE source = 'kitsu' AND task = 'sort:ID'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(err.is_none());
    }

    #[test]
    fn mark_error_truncates_a_huge_message() {
        // Upstream error strings embed response bodies; the column must not
        // grow without bound and the UI must not receive a megabyte of HTML.
        let c = conn();
        let huge = "e".repeat(5_000);
        mark_error(&c, "shikimori", "order:rating", &huge);
        let stored: String = c
            .query_row(
                "SELECT last_error FROM sync_state WHERE source = 'shikimori'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored.chars().count(), 400);
    }

    #[test]
    fn mark_error_creates_the_row_when_the_task_is_new() {
        let c = conn();
        mark_error(&c, "anilist", "sort:TITLE_ROMAJI", "boom");
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM sync_state", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn mark_started_is_idempotent() {
        let c = conn();
        mark_started(&c, "anilist").unwrap();
        mark_started(&c, "anilist").unwrap();
        let n: i64 = c
            .query_row("SELECT COUNT(*) FROM sync_state WHERE task = 'run'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(n, 1);
    }

    // --------------------------------------------------------------- fts

    #[test]
    fn rebuild_fts_indexes_every_title_variant() {
        let c = conn();
        insert_anime(&c, "al:1", Some("Shingeki no Kyojin"));
        c.execute(
            "UPDATE anime SET title_russian = 'Атака Титанов' WHERE uid = 'al:1'",
            [],
        )
        .unwrap();
        assert_eq!(rebuild_fts(&c).unwrap(), 1);

        // The whole point of unicode61: a Cyrillic query must find a title that
        // plain LIKE could never match, because LIKE folds ASCII only.
        let hit: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM anime_fts WHERE anime_fts MATCH '\"атака\"*'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hit, 1);
    }

    #[test]
    fn rebuild_fts_replaces_instead_of_duplicating() {
        let c = conn();
        insert_anime(&c, "al:1", Some("One"));
        rebuild_fts(&c).unwrap();
        rebuild_fts(&c).unwrap();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM anime_fts", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn fts_misses_titles_that_have_no_text_at_all() {
        // A row with every title NULL must still be indexed (or at least not
        // break the rebuild) — rows like that exist for id-only imports.
        let c = conn();
        insert_anime(&c, "sh:7", None);
        assert_eq!(rebuild_fts(&c).unwrap(), 1);
    }

    // ------------------------------------------------------------- schema

    #[test]
    fn schema_is_idempotent() {
        // `Db::open` is called once per process, but a second process on the
        // same file must not fail: every statement is IF NOT EXISTS.
        let c = conn();
        apply_schema_on(&c).expect("second apply");
        let v: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
    }

    #[test]
    fn schema_keeps_unicode_slugs_unique() {
        // The unique index on `genres.slug` is what makes genre matching
        // idempotent, so two different labels that normalise to one slug must
        // collapse rather than raise.
        let c = conn();
        c.execute(
            "INSERT INTO genres (slug, name_en, created_at) VALUES ('action', 'Action', 1)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO genres (slug, name_en, created_at)
             VALUES ('action', 'ACTION', 1)
             ON CONFLICT(slug) DO UPDATE SET name_en = excluded.name_en",
            [],
        )
        .unwrap();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM genres", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn count_reads_an_arbitrary_table() {
        let c = conn();
        assert_eq!(count(&c, "anime").unwrap(), 0);
        insert_anime(&c, "al:1", Some("One"));
        insert_anime(&c, "al:2", Some("Two"));
        assert_eq!(count(&c, "anime").unwrap(), 2);
    }

    #[test]
    fn anime_ids_from_different_sources_cannot_collide() {
        // The whole reason the primary key is a text uid: v1 used a signed
        // integer and a Kitsu-only row could land on the number an AniList row
        // already used.
        let c = conn();
        c.execute(
            "INSERT INTO anime (uid, anilist_id, created_at) VALUES ('al:1', 1, 1)",
            [],
        )
        .unwrap();
        c.execute(
            "INSERT INTO anime (uid, kitsu_id, created_at) VALUES ('ks:1', 1, 1)",
            [],
        )
        .unwrap();
        assert_eq!(count(&c, "anime").unwrap(), 2);
    }

    #[test]
    fn the_same_anilist_id_cannot_be_stored_twice() {
        let c = conn();
        c.execute(
            "INSERT INTO anime (uid, anilist_id, created_at) VALUES ('al:1', 42, 1)",
            [],
        )
        .unwrap();
        let second = c.execute(
            "INSERT INTO anime (uid, anilist_id, created_at) VALUES ('al:2', 42, 1)",
            [],
        );
        assert!(second.is_err(), "anilist_id is UNIQUE by design");
    }
}
