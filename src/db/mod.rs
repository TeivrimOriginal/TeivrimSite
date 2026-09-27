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
    let fts = create_fts(&conn).unwrap_or_else(|e| {
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
        let total = count(&conn, "anime")?;
        if indexed != total {
            log_info(&format!(
                "[db] пересобираю поисковый индекс ({} из {} записей)",
                indexed, total
            ));
            if let Err(e) = rebuild_fts(&conn) {
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
