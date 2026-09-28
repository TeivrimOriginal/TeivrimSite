use rusqlite::Connection;
use std::path::Path;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// How long a caller waits for a connection before giving up.
///
/// Waiting without a bound is what turns a slow query into a hung server:
/// actix runs a fixed number of worker threads and a thread parked in `get`
/// serves nothing at all, including the health probe. Ten seconds is longer
/// than any query here and shorter than the 30 second client timeout, so an
/// overloaded server answers with an error instead of a dropped connection.
///
/// It is also what makes [`PoolError::Timeout`] reachable at all: the parameter
/// it was written for was only ever passed `false`.
const WAIT_TIMEOUT: Duration = Duration::from_secs(10);

/// A small fixed-size connection pool for SQLite.
///
/// SQLite does not benefit from a huge pool: reads are cheap and concurrent
/// thanks to WAL, and there is exactly one writer at a time no matter how many
/// connections exist. So a handful of connections is the right shape, and it
/// removes a heavyweight async-pool dependency from the build.
pub struct Pool {
    idle: Mutex<Vec<Connection>>,
    ready: Condvar,
}

pub struct PooledConn<'a> {
    pool: &'a Pool,
    conn: Option<Connection>,
}

impl std::ops::Deref for PooledConn<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.conn.as_ref().expect("connection already returned to pool")
    }
}

impl std::ops::DerefMut for PooledConn<'_> {
    fn deref_mut(&mut self) -> &mut Connection {
        self.conn.as_mut().expect("connection already returned to pool")
    }
}

impl Drop for PooledConn<'_> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            let mut idle = match self.pool.idle.lock() {
                Ok(g) => g,
                // A poisoned mutex only means some other request panicked while
                // holding it. The connection itself is still fine, so recover
                // the guard rather than poisoning the whole pool.
                Err(p) => p.into_inner(),
            };
            idle.push(conn);
            self.pool.ready.notify_one();
        }
    }
}

#[derive(Debug)]
pub enum PoolError {
    Timeout,
    Init(String),
}

impl std::fmt::Display for PoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PoolError::Timeout => write!(f, "timed out waiting for a database connection"),
            PoolError::Init(m) => write!(f, "database init failed: {}", m),
        }
    }
}

impl Pool {
    /// Opens `size` connections and applies the per-connection pragmas.
    pub fn new(path: impl AsRef<Path>, size: usize) -> Result<Pool, PoolError> {
        let size = size.max(1);
        let path = path.as_ref().to_string_lossy().to_string();
        let mut conns = Vec::with_capacity(size);
        for i in 0..size {
            let conn = Connection::open(path.as_str()).map_err(|e| PoolError::Init(e.to_string()))?;
            configure(&conn, i == 0).map_err(|e| PoolError::Init(e.to_string()))?;
            conns.push(conn);
        }
        Ok(Pool {
            idle: Mutex::new(conns),
            ready: Condvar::new(),
        })
    }


    /// Blocks until a connection is free, for at most [`WAIT_TIMEOUT`].
    pub fn get(&self) -> Result<PooledConn<'_>, PoolError> {
        self.take(WAIT_TIMEOUT)
    }


    /// Waits up to `deadline` for a connection.
    ///
    /// A zero `deadline` never blocks: it is the "try once" mode, used by the
    /// tests to reach the saturated branch without spending ten seconds in it.
    fn take(&self, deadline: Duration) -> Result<PooledConn<'_>, PoolError> {
        let mut idle = self.idle.lock().unwrap_or_else(|p| p.into_inner());
        let started = Instant::now();
        loop {
            if let Some(conn) = idle.pop() {
                return Ok(PooledConn {
                    pool: self,
                    conn: Some(conn),
                });
            }
            let left = deadline.saturating_sub(started.elapsed());
            if left.is_zero() {
                return Err(PoolError::Timeout);
            }
            let (guard, waited) = self
                .ready
                .wait_timeout(idle, left)
                .unwrap_or_else(|p| p.into_inner());
            idle = guard;
            if waited.timed_out() {
                // A connection can be returned in the very instant the wait
                // expires, so look once more before reporting the timeout.
                if let Some(conn) = idle.pop() {
                    return Ok(PooledConn {
                        pool: self,
                        conn: Some(conn),
                    });
                }
                return Err(PoolError::Timeout);
            }
        }
    }
}

/// Per-connection pragmas.
///
/// WAL is what actually makes concurrent reads work; `busy_timeout` covers the
/// brief window where a writer holds the lock; the cache size keeps the hot
/// index pages resident so paged listing stays fast.
///
/// `first` is used for `journal_mode` alone. Every other setting here lives in
/// the connection, not in the file, so setting it on one connection of the pool
/// leaves the other seven on SQLite's default — and the pool hands connections
/// out in reverse order of creation, so the configured one is also the last one
/// anybody gets.
fn configure(conn: &Connection, first: bool) -> Result<(), rusqlite::Error> {
    conn.busy_timeout(Duration::from_secs(30))?;
    if first {
        // journal_mode is persistent in the database file, so one connection
        // is enough — and one is right, because setting it takes a write lock
        // and every connection opening at once would contend for it.
        conn.pragma_update(None, "journal_mode", "WAL")?;
    }
    // NORMAL rather than FULL: in WAL mode FULL fsyncs on every commit, and
    // the loaders commit per row.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    // Set on every connection rather than trusted from the build: the bundled
    // SQLite happens to default it on, but every `ON DELETE CASCADE` in the
    // schema silently stops working if it is ever off.
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    // Negative values are KiB, so this is a 128 MiB page cache per connection.
    conn.pragma_update(None, "cache_size", -131_072i64)?;
    conn.pragma_update(None, "mmap_size", 268_435_456i64)?; // 256 MiB
    conn.pragma_update(None, "optimize", "")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
    use std::sync::Arc;
    use std::time::Duration;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// A file the test removes afterwards. `:memory:` will not do: every
    /// connection to a memory database gets a private one, so a pool of them
    /// would not see the same tables — and the pragmas under test are exactly
    /// the per-connection ones.
    struct TempDb(std::path::PathBuf);

    impl TempDb {
        fn new() -> TempDb {
            let n = COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
            TempDb(std::env::temp_dir().join(format!(
                "anime-pool-test-{}-{}.db",
                std::process::id(),
                n
            )))
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            for suffix in ["", "-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{}", self.0.display(), suffix));
            }
        }
    }

    fn pool(size: usize) -> (TempDb, Pool) {
        let db = TempDb::new();
        let p = Pool::new(&db.0, size).expect("pool");
        (db, p)
    }

    /// Takes every connection the pool has, which is the only way to look at
    /// more than one: the pool hands them out in reverse order of creation, so
    /// a sequence of `get` calls is the only view of "all of them" there is.
    ///
    /// The count is explicit because a pool of exactly N never returns an
    /// error on the N+1st call — it blocks, and a test that blocks is a hung
    /// test rather than a failing one.
    fn drain(pool: &Pool, count: usize) -> Vec<PooledConn<'_>> {
        (0..count).map(|_| pool.get().expect("connection")).collect()
    }

    fn pragma_int(conn: &Connection, name: &str) -> i64 {
        conn.query_row(&format!("PRAGMA {}", name), [], |r| r.get::<_, i64>(0))
            .unwrap_or(-1)
    }

    fn pragma_text(conn: &Connection, name: &str) -> String {
        conn.query_row(&format!("PRAGMA {}", name), [], |r| r.get::<_, String>(0))
            .unwrap_or_default()
    }

    // -------------------------------------------------------- the guard

    #[test]
    fn the_guard_hands_out_a_usable_connection() {
        // This is the test the two `expect` calls in `Deref`/`DerefMut` rest
        // on: a live guard must always carry a connection, and it must be one
        // that works through both deref paths.
        let (_db, p) = pool(1);
        let conn = p.get().expect("connection");
        conn.execute_batch("CREATE TABLE t (a INTEGER); INSERT INTO t VALUES (7);")
            .expect("write");
        let read: i64 = conn.query_row("SELECT a FROM t", [], |r| r.get(0)).expect("read");
        assert_eq!(read, 7);
    }

    #[test]
    fn a_connection_goes_back_to_the_pool_when_the_guard_drops() {
        // Without the return the second `get` would block forever, so this is
        // also the test that the `Drop` arm of the invariant is not dead code.
        let (_db, p) = pool(1);
        {
            let conn = p.get().expect("connection");
            conn.execute_batch("CREATE TABLE t (a INTEGER)").expect("write");
            assert_eq!(p.idle.lock().expect("lock").len(), 0, "связь взята из пула");
        }
        assert_eq!(p.idle.lock().expect("lock").len(), 1, "связь не вернулась в пул");
        // The very same connection comes back, not a fresh one: the table it
        // created is still there.
        let conn = p.get().expect("connection");
        let tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 't'",
                [],
                |r| r.get(0),
            )
            .expect("table survives");
        assert_eq!(tables, 1, "пул отдал не то же соединение, а новое");
    }

    #[test]
    fn a_pool_of_one_hands_the_connection_to_a_second_caller_only_after_the_first_is_done() {
        // The blocking behaviour is the point of the condvar, and it is what
        // `PoolError::Timeout` is the escape hatch for.
        let (_db, owned) = pool(1);
        let p = Arc::new(owned);
        let first = p.get().expect("first");
        let other = Arc::clone(&p);
        let waiter = std::thread::spawn(move || other.get().is_ok());
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(p.idle.lock().expect("lock").len(), 0);
        drop(first);
        assert!(waiter.join().expect("waiter"), "второй должен дождаться освобождения");
    }

    #[test]
    fn a_saturated_pool_reports_a_timeout_instead_of_waiting_forever() {
        // The wait has a bound on purpose: a parked actix worker serves
        // nothing, so an overloaded server has to answer rather than hang.
        let (_db, p) = pool(1);
        let held = p.get().expect("связь");
        let started = Instant::now();
        let err = p.take(Duration::from_millis(50)).err();
        assert!(matches!(err, Some(PoolError::Timeout)), "ошибка: {:?}", err);
        assert!(started.elapsed() < Duration::from_secs(5), "ждал {:?}", started.elapsed());
        drop(held);
    }

    #[test]
    fn a_zero_deadline_never_waits_at_all() {
        // The "try once" mode, used by the saturated test above so it costs
        // milliseconds rather than the production ten seconds.
        let (_db, p) = pool(1);
        let _held = p.get().expect("связь");
        let started = Instant::now();
        assert!(p.take(Duration::ZERO).is_err());
        assert!(started.elapsed() < Duration::from_millis(50));
    }

    #[test]
    fn a_returned_connection_wakes_the_waiter_up() {
        // `wait_timeout` must still be woken by `notify_one` and not only by
        // the clock, or every request would pay the full deadline.
        let (_db, owned) = pool(1);
        let p = Arc::new(owned);
        let held = p.get().expect("связь");
        let other = Arc::clone(&p);
        let waiter = std::thread::spawn(move || other.take(Duration::from_secs(5)).is_ok());
        std::thread::sleep(Duration::from_millis(50));
        let started = Instant::now();
        drop(held);
        assert!(waiter.join().expect("waiter"));
        assert!(started.elapsed() < Duration::from_secs(4), "проснулся только по таймауту");
    }

    #[test]
    fn the_production_wait_is_bounded_and_not_instant() {
        // Both extremes are wrong: zero would turn every contended read into a
        // 500, and unbounded would park the worker forever.
        assert!(WAIT_TIMEOUT > Duration::ZERO);
        assert!(WAIT_TIMEOUT < Duration::from_secs(30));
    }

    #[test]
    fn a_zero_sized_pool_is_raised_to_one() {
        // `DB_POOL_SIZE=0` in the environment must not produce a pool that can
        // never hand anything out.
        let (_db, p) = pool(0);
        assert!(p.get().is_ok());
    }

    #[test]
    fn a_failure_to_open_is_reported_not_swallowed() {
        // A path under a directory that does not exist cannot become a
        // database, and `main` has to see that at boot rather than hours later
        // inside a loader.
        let n = COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
        let missing = std::env::temp_dir().join(format!("anime-pool-missing-{}", n)).join("a.db");
        let err = Pool::new(&missing, 1).err();
        assert!(matches!(err, Some(PoolError::Init(_))), "ошибка: {:?}", err);
    }

    // ------------------------------------------------------ poisoning

    /// Panics while holding the idle mutex, which is the only way a pool lock
    /// becomes poisoned, and reports whether it is now poisoned.
    fn poison(pool: &Pool) -> bool {
        let held = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = pool.idle.lock().expect("lock");
            panic!("deliberate panic while holding the pool lock");
        }));
        assert!(held.is_err(), "паника должна была произойти");
        pool.idle.is_poisoned()
    }

    #[test]
    fn a_poisoned_pool_still_hands_out_connections() {
        // The recovery is the reason `take` and `Drop` use `unwrap_or_else`
        // instead of `unwrap`: one panicking request must not take the whole
        // database down for the life of the process.
        let (_db, p) = pool(1);
        assert!(poison(&p), "мьютекс должен быть отравлен");
        let conn = p.get().expect("связь после отравления");
        let n: i64 = conn.query_row("SELECT 1", [], |r| r.get(0)).expect("запрос");
        assert_eq!(n, 1);
    }

    #[test]
    fn a_connection_is_still_returned_into_a_poisoned_pool() {
        // The other half: the guard is dropped after the pool is poisoned, and
        // that drop must not panic — a panic in `Drop` during unwinding aborts
        // the process instead of unwinding into it.
        let (_db, p) = pool(1);
        let conn = p.get().expect("связь");
        assert!(poison(&p));
        drop(conn);
        assert_eq!(p.idle.lock().unwrap_or_else(|e| e.into_inner()).len(), 1);
    }

    // -------------------------------------------------------- pragmas

    #[test]
    fn every_pooled_connection_gets_the_per_connection_pragmas() {
        // `foreign_keys` and `synchronous` live in the connection, not in the
        // file. Setting them on one connection out of the pool leaves the
        // others quietly not enforcing the cascades and fsyncing on every
        // write, and the bug is invisible: everything still works.
        let (_db, p) = pool(4);
        let held = drain(&p, 4);
        for (i, conn) in held.iter().enumerate() {
            assert_eq!(pragma_int(conn, "foreign_keys"), 1, "соединение {} без foreign_keys", i);
            assert_eq!(pragma_int(conn, "synchronous"), 1, "соединение {} не на NORMAL", i);
            assert_eq!(pragma_int(conn, "busy_timeout"), 30_000, "соединение {}", i);
            assert!(pragma_int(conn, "cache_size") < 0, "соединение {} без кэша", i);
        }
    }

    #[test]
    fn the_file_is_in_wal_mode() {
        // `journal_mode` is persistent, so setting it once is enough — and it
        // is the one pragma that is genuinely shared.
        let (_db, p) = pool(1);
        let conn = p.get().expect("связь");
        assert_eq!(pragma_text(&conn, "journal_mode").to_lowercase(), "wal");
    }

    #[test]
    fn a_delete_cascades_on_every_connection_of_the_pool() {
        // The behaviour the pragma enables, asserted through the schema rather
        // than through the pragma, so it stays true even if a future SQLite
        // build flips its default.
        let db = crate::db::testing::test_db();
        let conns = drain_of_handle(&db.handle, 2);

        for (i, conn) in conns.iter().enumerate() {
            conn.execute(
                "INSERT INTO users (id, username, username_key, password_hash, created_at)
                 VALUES (?1, ?2, ?3, 'x', 1)",
                rusqlite::params![100 + i as i64, format!("u{}", i), format!("u{}", i)],
            )
            .expect("user");
            conn.execute(
                "INSERT INTO auth_tokens (token_hash, user_id, created_at, expires_at)
                 VALUES (?1, ?2, 1, 9999999999)",
                rusqlite::params![format!("h{}", i), 100 + i as i64],
            )
            .expect("token");
        }

        for (i, conn) in conns.iter().enumerate() {
            conn.execute("DELETE FROM users WHERE id = ?1", [100 + i as i64])
                .expect("delete user");
            let left: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM auth_tokens WHERE user_id = ?1",
                    [100 + i as i64],
                    |r| r.get(0),
                )
                .expect("count");
            assert_eq!(left, 0, "соединение {} оставило осиротевшую сессию", i);
        }
    }

    fn drain_of_handle(handle: &crate::db::Handle, count: usize) -> Vec<PooledConn<'_>> {
        (0..count).map(|_| handle.conn().expect("связь из пула")).collect()
    }
}
