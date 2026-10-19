//! [`wp_economy::AccountStore`] over the meta database.
//!
//! `econ_accounts` holds the latest `PlayerAccount` (JSON, with cash/rep/bounty/cop rank copied
//! into columns for queries); `econ_events` is the append-only `EconomyEvent` archive. A commit
//! checks that the events continue the stored sequence with no gaps (same rule as the economy's
//! own stores), so two instances cannot both commit on top of the same version of an account.

use std::sync::Arc;

use axum::http::StatusCode;
use rusqlite::{params, Connection, OptionalExtension};
use wp_contracts::PlayerId;
use wp_economy::{AccountStore, EconomyEvent, PlayerAccount, StoreError};

use crate::clock::{Clock, UnixMs};
use crate::db::Db;
use crate::error::ApiError;

fn backend(e: rusqlite::Error) -> StoreError {
    StoreError::Backend(e.to_string())
}

fn corrupt(e: serde_json::Error) -> StoreError {
    StoreError::Corrupt(e.to_string())
}

/// Map a store error to an HTTP error (sequence conflicts are 409 `seq_conflict`).
pub fn store_error(e: StoreError) -> ApiError {
    match e {
        StoreError::SeqConflict { .. } => {
            ApiError::new(StatusCode::CONFLICT, "seq_conflict", e.to_string())
        }
        StoreError::Backend(_) | StoreError::Corrupt(_) => ApiError::internal(e.to_string()),
    }
}

/// Next expected event sequence number of an account (0 when it has no events).
pub fn next_seq(conn: &Connection, player: PlayerId) -> Result<u64, StoreError> {
    let max: Option<i64> = conn
        .query_row(
            "SELECT MAX(seq) FROM econ_events WHERE account_id = ?1",
            params![player.0],
            |r| r.get(0),
        )
        .map_err(backend)?;
    Ok(max.map_or(0, |s| s as u64 + 1))
}

/// The economy's sequence rule: events are `next_stored, next_stored+1, ...` for this player
/// and the account's `next_event_seq` follows the last one.
pub fn check_sequence(
    next_stored: u64,
    account: &PlayerAccount,
    events: &[EconomyEvent],
) -> Result<(), StoreError> {
    let mut expected = next_stored;
    for e in events {
        if e.seq != expected || e.player != account.player {
            return Err(StoreError::SeqConflict {
                expected,
                got: e.seq,
            });
        }
        expected += 1;
    }
    if account.next_event_seq != expected {
        return Err(StoreError::SeqConflict {
            expected,
            got: account.next_event_seq,
        });
    }
    Ok(())
}

/// Load an account's economy state.
pub fn load(conn: &Connection, player: PlayerId) -> Result<Option<PlayerAccount>, StoreError> {
    let json: Option<String> = conn
        .query_row(
            "SELECT json FROM econ_accounts WHERE account_id = ?1",
            params![player.0],
            |r| r.get(0),
        )
        .optional()
        .map_err(backend)?;
    json.map(|j| serde_json::from_str(&j).map_err(corrupt))
        .transpose()
}

/// Commit inside an existing transaction (the caller commits it).
pub fn commit_tx(
    conn: &Connection,
    account: &PlayerAccount,
    events: &[EconomyEvent],
    now: UnixMs,
) -> Result<(), StoreError> {
    check_sequence(next_seq(conn, account.player)?, account, events)?;
    for e in events {
        let j = serde_json::to_string(e).map_err(corrupt)?;
        conn.execute(
            "INSERT INTO econ_events (account_id, seq, kind, json, recorded_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![account.player.0, e.seq as i64, e.reason.kind_code(), j, now],
        )
        .map_err(backend)?;
    }
    let j = serde_json::to_string(account).map_err(corrupt)?;
    conn.execute(
        "INSERT INTO econ_accounts
             (account_id, json, cash, rep, bounty, cop_rank, next_seq, updated_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(account_id) DO UPDATE SET
             json = excluded.json, cash = excluded.cash, rep = excluded.rep,
             bounty = excluded.bounty, cop_rank = excluded.cop_rank,
             next_seq = excluded.next_seq, updated_ms = excluded.updated_ms",
        params![
            account.player.0,
            j,
            account.cash,
            account.rep,
            account.bounty,
            account.cop_rank,
            account.next_event_seq as i64,
            now
        ],
    )
    .map_err(backend)?;
    Ok(())
}

/// Commit in its own transaction.
pub fn commit(
    conn: &mut Connection,
    account: &PlayerAccount,
    events: &[EconomyEvent],
    now: UnixMs,
) -> Result<(), StoreError> {
    let tx = conn.transaction().map_err(backend)?;
    commit_tx(&tx, account, events, now)?;
    tx.commit().map_err(backend)
}

/// Events with `seq >= from_seq`, in order.
pub fn events_since(
    conn: &Connection,
    player: PlayerId,
    from_seq: u64,
) -> Result<Vec<EconomyEvent>, StoreError> {
    let mut stmt = conn
        .prepare("SELECT json FROM econ_events WHERE account_id = ?1 AND seq >= ?2 ORDER BY seq")
        .map_err(backend)?;
    let rows = stmt
        .query_map(params![player.0, from_seq as i64], |r| {
            r.get::<_, String>(0)
        })
        .map_err(backend)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(serde_json::from_str(&r.map_err(backend)?).map_err(corrupt)?);
    }
    Ok(out)
}

/// In-process [`AccountStore`] over the meta database (for code linked into the meta service,
/// e.g. tests or a co-located server). Remote servers use
/// [`crate::client::RemoteAccountStore`].
#[derive(Clone)]
pub struct MetaStore {
    db: Db,
    clock: Arc<dyn Clock>,
}

impl MetaStore {
    /// Wrap a database handle.
    pub fn new(db: Db, clock: Arc<dyn Clock>) -> Self {
        Self { db, clock }
    }
}

impl AccountStore for MetaStore {
    fn load(&self, player: PlayerId) -> Result<Option<PlayerAccount>, StoreError> {
        load(&self.db.lock(), player)
    }

    fn commit(
        &mut self,
        account: &PlayerAccount,
        events: &[EconomyEvent],
    ) -> Result<(), StoreError> {
        let now = self.clock.now_ms();
        commit(&mut self.db.lock(), account, events, now)
    }

    fn events_since(
        &self,
        player: PlayerId,
        from_seq: u64,
    ) -> Result<Vec<EconomyEvent>, StoreError> {
        events_since(&self.db.lock(), player, from_seq)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ManualClock;
    use wp_economy::Economy;

    fn store_with_account(id: u32) -> MetaStore {
        let db = Db::open_in_memory().unwrap();
        db.lock()
            .execute(
                "INSERT INTO accounts (id, display_name, created_ms) VALUES (?1, ?2, 0)",
                params![id, format!("p{id}")],
            )
            .unwrap();
        MetaStore::new(db, Arc::new(ManualClock::new(0)))
    }

    /// Same contract the economy's own stores satisfy (crates/wp_economy/tests/stores.rs).
    #[test]
    fn account_store_contract() {
        let eco = Economy::embedded();
        let mut store = store_with_account(7);
        let p = PlayerId(7);
        assert_eq!(store.load(p).unwrap(), None);

        let (mut acct, ev) = eco.open_account(p);
        store.commit(&acct, &ev).unwrap();
        assert_eq!(store.load(p).unwrap().as_ref(), Some(&acct));

        let ((), ev2) = eco.adjust(&mut acct, 500, "test").unwrap();
        store.commit(&acct, &ev2).unwrap();
        let all = store.events_since(p, 0).unwrap();
        assert_eq!(all.len(), ev.len() + ev2.len());
        assert_eq!(store.events_since(p, ev.len() as u64).unwrap(), ev2);

        // Replaying the same commit is a sequence conflict, and nothing changes.
        let err = store.commit(&acct, &ev2).unwrap_err();
        assert!(matches!(err, StoreError::SeqConflict { .. }));
        assert_eq!(store.events_since(p, 0).unwrap().len(), all.len());
        assert_eq!(store.load(p).unwrap().unwrap().cash, acct.cash);
    }

    #[test]
    fn commit_for_unknown_account_fails() {
        let eco = Economy::embedded();
        let mut store = store_with_account(1);
        let (acct, ev) = eco.open_account(PlayerId(99));
        assert!(matches!(
            store.commit(&acct, &ev),
            Err(StoreError::Backend(_))
        ));
    }
}
