use rusqlite::Connection;

pub const DB_FILE: &str = "AnimeData.db";

pub fn open() -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open(DB_FILE)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA cache_size = -128000;
         PRAGMA temp_store = MEMORY;
         PRAGMA busy_timeout = 15000;",
    )?;
    Ok(conn)
}

pub fn ensure_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS anime (
            anilist_id            INTEGER PRIMARY KEY,
            mal_id                INTEGER,
            shikimori_id          INTEGER,
            kitsu_id              TEXT,

            title_romaji          TEXT,
            title_english         TEXT,
            title_native          TEXT,
            title_user_preferred  TEXT,
            title_russian         TEXT,
            title_ru_machine      TEXT,
            synonyms              TEXT,

            format                TEXT,
            status                TEXT,
            description           TEXT,
            description_ru        TEXT,
            description_ru_machine TEXT,
            duration              INTEGER,
            episodes              INTEGER,
            chapters              INTEGER,
            volumes               INTEGER,
            country_of_origin     TEXT,
            is_adult              INTEGER,
            is_licensed           INTEGER,

            start_year            INTEGER,
            start_month           INTEGER,
            start_day             INTEGER,
            end_year              INTEGER,
            end_month             INTEGER,
            end_day               INTEGER,
            season                TEXT,
            season_year           INTEGER,

            average_score         INTEGER,
            mean_score            INTEGER,
            popularity            INTEGER,
            favourites            INTEGER,
            trending              INTEGER,
            mal_score             REAL,
            mal_rank              INTEGER,
            mal_members           INTEGER,

            cover_extra_large     TEXT,
            cover_large           TEXT,
            cover_medium          TEXT,
            cover_color           TEXT,
            banner_image          TEXT,

            trailer_id            TEXT,
            trailer_site          TEXT,
            trailer_thumbnail     TEXT,

            genres_json           TEXT,
            tags_json             TEXT,
            studios_json          TEXT,
            relations_json        TEXT,
            external_links_json   TEXT,
            streaming_episodes_json TEXT,
            recommendations_json  TEXT,

            jikan_genres_json     TEXT,
            jikan_themes_json     TEXT,
            jikan_demographics_json TEXT,
            jikan_studios_json    TEXT,
            jikan_producers_json  TEXT,
            jikan_licensors_json  TEXT,
            jikan_staff_json      TEXT,
            jikan_characters_json TEXT,

            shikimori_studios_json TEXT,

            updated_at            INTEGER DEFAULT (strftime('%s','now')),
            anilist_synced_at     INTEGER,
            jikan_synced_at       INTEGER,
            shikimori_synced_at   INTEGER,
            translated_at         INTEGER,
            genres_matched_at     INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_mal_id        ON anime(mal_id);
        CREATE INDEX IF NOT EXISTS idx_shikimori_id  ON anime(shikimori_id);
        CREATE INDEX IF NOT EXISTS idx_title_romaji  ON anime(title_romaji);
        CREATE INDEX IF NOT EXISTS idx_title_russian ON anime(title_russian);
        CREATE INDEX IF NOT EXISTS idx_popularity    ON anime(popularity DESC);
        CREATE INDEX IF NOT EXISTS idx_score         ON anime(average_score DESC);
        CREATE INDEX IF NOT EXISTS idx_mal_score     ON anime(mal_score DESC);
        CREATE INDEX IF NOT EXISTS idx_format        ON anime(format);
        CREATE INDEX IF NOT EXISTS idx_status        ON anime(status);

        CREATE TABLE IF NOT EXISTS sync_state (
            source      TEXT NOT NULL,
            task        TEXT NOT NULL,
            last_page   INTEGER DEFAULT 0,
            last_id     TEXT,
            total_saved INTEGER DEFAULT 0,
            finished    INTEGER DEFAULT 0,
            updated_at  INTEGER DEFAULT (strftime('%s','now')),
            PRIMARY KEY (source, task)
        );

        CREATE TABLE IF NOT EXISTS genres_dict (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            name_en       TEXT NOT NULL UNIQUE,
            name_ru       TEXT,
            category      TEXT,
            auto_created  INTEGER DEFAULT 0,
            created_at    INTEGER DEFAULT (strftime('%s','now'))
        );

        CREATE INDEX IF NOT EXISTS idx_genres_en ON genres_dict(name_en);
        CREATE INDEX IF NOT EXISTS idx_genres_ru ON genres_dict(name_ru);

        CREATE TABLE IF NOT EXISTS anime_genres (
            anilist_id  INTEGER NOT NULL,
            genre_id    INTEGER NOT NULL,
            source      TEXT,
            PRIMARY KEY (anilist_id, genre_id)
        );

        CREATE INDEX IF NOT EXISTS idx_ag_anime  ON anime_genres(anilist_id);
        CREATE INDEX IF NOT EXISTS idx_ag_genre  ON anime_genres(genre_id);
        "#,
    )
}

pub fn get_checkpoint(conn: &Connection, source: &str, task: &str) -> (i64, bool) {
    conn.query_row(
        "SELECT COALESCE(last_page, 0), COALESCE(finished, 0) FROM sync_state WHERE source = ?1 AND task = ?2",
        rusqlite::params![source, task],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? != 0)),
    )
    .unwrap_or((0, false))
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
        "INSERT OR REPLACE INTO sync_state (source, task, last_page, total_saved, finished, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, strftime('%s','now'))",
        rusqlite::params![source, task, last_page, total_saved, if finished { 1 } else { 0 }],
    )?;
    Ok(())
}