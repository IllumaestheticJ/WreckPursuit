//! Database handle and the embedded, ordered migration runner.
//!
//! All SQL lives in this crate's modules and in `migrations/*.sql`. Migrations are applied in
//! file order inside one transaction each and recorded in `schema_migrations`; an applied
//! migration is never edited — add a new file instead.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{params, Connection};

/// Embedded migrations, in order. `(version, name, sql)`.
pub const MIGRATIONS: &[(i64, &str, &str)] = &[
    (
        1,
        "accounts_auth",
        include_str!("../migrations/0001_accounts_auth.sql"),
    ),
    (
        2,
        "progression",
        include_str!("../migrations/0002_progression.sql"),
    ),
    (
        3,
        "directory_matchmaking",
        include_str!("../migrations/0003_directory_matchmaking.sql"),
    ),
    (
        4,
        "leaderboards_admin",
        include_str!("../migrations/0004_leaderboards_admin.sql"),
    ),
];

/// Shared database handle. SQLite has a single writer anyway; one connection behind a mutex
/// keeps the code simple. Hold the guard only for synchronous work (never across `.await`).
#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// Open (or create) a database file and run pending migrations.
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        Self::init(Connection::open(path)?)
    }

    /// Fresh in-memory database (tests), migrated.
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;",
        )?;
        migrate(&mut conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Lock the connection.
    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves SQLite consistent (transactions roll back on
        // drop), so a poisoned lock is safe to reuse.
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// Apply every migration newer than the recorded schema version. Returns how many ran.
pub fn migrate(conn: &mut Connection) -> rusqlite::Result<usize> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
             version INTEGER PRIMARY KEY,
             name    TEXT NOT NULL
         );",
    )?;
    let current: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )?;
    let mut ran = 0;
    for (version, name, sql) in MIGRATIONS {
        if *version <= current {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute(
            "INSERT INTO schema_migrations (version, name) VALUES (?1, ?2)",
            params![version, name],
        )?;
        tx.commit()?;
        ran += 1;
    }
    Ok(ran)
}

/// Current schema version (highest applied migration).
pub fn schema_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_once_and_in_order() {
        let mut conn = Connection::open_in_memory().unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), MIGRATIONS.len());
        assert_eq!(migrate(&mut conn).unwrap(), 0);
        assert_eq!(schema_version(&conn).unwrap(), MIGRATIONS.last().unwrap().0);
        for (i, (v, _, _)) in MIGRATIONS.iter().enumerate() {
            assert_eq!(*v, i as i64 + 1, "versions are 1..=n with no gaps");
        }
    }

    #[test]
    fn archives_are_append_only() {
        let db = Db::open_in_memory().unwrap();
        let c = db.lock();
        c.execute(
            "INSERT INTO audit_log (at_ms, actor, action, target, details) VALUES (1,'a','b','c','{}')",
            [],
        )
        .unwrap();
        assert!(c.execute("DELETE FROM audit_log", []).is_err());
        assert!(c.execute("UPDATE audit_log SET actor = 'x'", []).is_err());
    }
}
