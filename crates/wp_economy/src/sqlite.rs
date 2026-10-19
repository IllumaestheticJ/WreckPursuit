//! SQLite [`AccountStore`] for the native server (`native` feature).
//!
//! Two tables: `accounts(player, json)` holds the latest account state, `events(player, seq,
//! kind, json)` the append-only audit log. A commit writes both in one transaction and checks
//! the event sequence continues the stored log (rejects lost updates / double commits).

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use wp_contracts::PlayerId;

use crate::ledger::{check_sequence, AccountStore, EconomyEvent, PlayerAccount, StoreError};

fn backend(e: rusqlite::Error) -> StoreError {
    StoreError::Backend(e.to_string())
}

/// SQLite-backed account store.
pub struct SqliteStore {
    conn: Connection,
}

impl SqliteStore {
    /// Open (or create) a database file.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Self::init(Connection::open(path).map_err(backend)?)
    }

    /// In-memory database (tests).
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory().map_err(backend)?)
    }

    fn init(conn: Connection) -> Result<Self, StoreError> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS accounts (
                 player INTEGER PRIMARY KEY,
                 json   TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS events (
                 player INTEGER NOT NULL,
                 seq    INTEGER NOT NULL,
                 kind   INTEGER NOT NULL,
                 json   TEXT NOT NULL,
                 PRIMARY KEY (player, seq)
             );",
        )
        .map_err(backend)?;
        Ok(Self { conn })
    }

    fn next_seq(&self, player: PlayerId) -> Result<u64, StoreError> {
        let n: Option<i64> = self
            .conn
            .query_row(
                "SELECT MAX(seq) FROM events WHERE player = ?1",
                params![player.0],
                |r| r.get(0),
            )
            .map_err(backend)?;
        Ok(n.map_or(0, |s| s as u64 + 1))
    }
}

impl AccountStore for SqliteStore {
    fn load(&self, player: PlayerId) -> Result<Option<PlayerAccount>, StoreError> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT json FROM accounts WHERE player = ?1",
                params![player.0],
                |r| r.get(0),
            )
            .optional()
            .map_err(backend)?;
        json.map(|j| serde_json::from_str(&j).map_err(|e| StoreError::Corrupt(e.to_string())))
            .transpose()
    }

    fn commit(
        &mut self,
        account: &PlayerAccount,
        events: &[EconomyEvent],
    ) -> Result<(), StoreError> {
        check_sequence(self.next_seq(account.player)?, account, events)?;
        let acct_json =
            serde_json::to_string(account).map_err(|e| StoreError::Corrupt(e.to_string()))?;
        let tx = self.conn.transaction().map_err(backend)?;
        for e in events {
            let j = serde_json::to_string(e).map_err(|e| StoreError::Corrupt(e.to_string()))?;
            tx.execute(
                "INSERT INTO events (player, seq, kind, json) VALUES (?1, ?2, ?3, ?4)",
                params![account.player.0, e.seq as i64, e.reason.kind_code(), j],
            )
            .map_err(backend)?;
        }
        tx.execute(
            "INSERT INTO accounts (player, json) VALUES (?1, ?2)
             ON CONFLICT(player) DO UPDATE SET json = excluded.json",
            params![account.player.0, acct_json],
        )
        .map_err(backend)?;
        tx.commit().map_err(backend)
    }

    fn events_since(
        &self,
        player: PlayerId,
        from_seq: u64,
    ) -> Result<Vec<EconomyEvent>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT json FROM events WHERE player = ?1 AND seq >= ?2 ORDER BY seq")
            .map_err(backend)?;
        let rows = stmt
            .query_map(params![player.0, from_seq as i64], |r| {
                r.get::<_, String>(0)
            })
            .map_err(backend)?;
        let mut out = Vec::new();
        for r in rows {
            let j = r.map_err(backend)?;
            out.push(serde_json::from_str(&j).map_err(|e| StoreError::Corrupt(e.to_string()))?);
        }
        Ok(out)
    }
}
