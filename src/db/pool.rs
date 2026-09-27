use rusqlite::Connection;
use std::path::Path;
use std::sync::{Condvar, Mutex};

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


    /// Blocks until a connection is free.
    pub fn get(&self) -> Result<PooledConn<'_>, PoolError> {
        self.take(false)
    }


    fn take(&self, fail_fast: bool) -> Result<PooledConn<'_>, PoolError> {
        let mut idle = self.idle.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if let Some(conn) = idle.pop() {
                return Ok(PooledConn {
                    pool: self,
                    conn: Some(conn),
                });
            }
            if fail_fast {
                return Err(PoolError::Timeout);
            }
            idle = self
                .ready
                .wait(idle)
                .unwrap_or_else(|p| p.into_inner());
        }
    }
}

/// Per-connection pragmas.
///
/// WAL is what actually makes concurrent reads work; `busy_timeout` covers the
/// brief window where a writer holds the lock; the cache size keeps the hot
/// index pages resident so paged listing stays fast.
fn configure(conn: &Connection, first: bool) -> Result<(), rusqlite::Error> {
    conn.busy_timeout(std::time::Duration::from_secs(30))?;
    if first {
        // journal_mode is persistent, so only one connection needs to set it.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
    }
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    // Negative values are KiB, so this is a 128 MiB page cache per connection.
    conn.pragma_update(None, "cache_size", -131_072i64)?;
    conn.pragma_update(None, "mmap_size", 268_435_456i64)?; // 256 MiB
    conn.pragma_update(None, "optimize", "")?;
    Ok(())
}
