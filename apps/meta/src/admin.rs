//! Moderation and operations: bans, kicks, audited economy adjustments, feature flags,
//! leaderboard resets and the audit log. Every mutating admin action writes one
//! `audit_log` row in the same transaction as the change.

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;
use wp_contracts::PlayerId;
use wp_economy::Economy;

use crate::api::{AccountId, AdminAccountView, AuditEntry, HealthReport};
use crate::auth;
use crate::clock::UnixMs;
use crate::directory;
use crate::error::{ApiError, ApiResult};
use crate::leaderboards::{self, Period};
use crate::store;

/// Permanent ban marker.
pub const PERMANENT: i64 = i64::MAX;

/// Append an audit row.
pub fn audit(
    conn: &Connection,
    actor: &str,
    action: &str,
    target: &str,
    details: serde_json::Value,
    now: UnixMs,
) -> ApiResult<()> {
    conn.execute(
        "INSERT INTO audit_log (at_ms, actor, action, target, details) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![now, actor, action, target, details.to_string()],
    )?;
    Ok(())
}

fn require_account(conn: &Connection, id: AccountId) -> ApiResult<()> {
    auth::profile(conn, id).map(|_| ())
}

fn require_reason(reason: &str) -> ApiResult<()> {
    if reason.trim().is_empty() || reason.len() > 500 {
        Err(ApiError::bad_request(
            "reason/note must be 1-500 characters",
        ))
    } else {
        Ok(())
    }
}

/// Ban an account (sessions revoked, pending kick queued so instances drop the player).
pub fn ban(
    conn: &mut Connection,
    actor: &str,
    id: AccountId,
    reason: &str,
    duration_s: Option<i64>,
    now: UnixMs,
) -> ApiResult<i64> {
    require_reason(reason)?;
    let until = match duration_s {
        None => PERMANENT,
        Some(s) if s > 0 => now.saturating_add(s.saturating_mul(1000)),
        Some(_) => return Err(ApiError::bad_request("duration_s must be positive")),
    };
    let tx = conn.transaction()?;
    require_account(&tx, id)?;
    tx.execute(
        "UPDATE accounts SET banned_until_ms = ?2, ban_reason = ?3 WHERE id = ?1",
        params![id, until, reason],
    )?;
    auth::revoke_all_sessions(&tx, id)?;
    tx.execute(
        "UPDATE tickets SET state = 'cancelled', join_token_hash = NULL
         WHERE account_id = ?1 AND state IN ('queued', 'assigned')",
        params![id],
    )?;
    tx.execute(
        "DELETE FROM reconnect_tokens WHERE account_id = ?1",
        params![id],
    )?;
    tx.execute(
        "INSERT INTO kicks (account_id, reason, created_ms) VALUES (?1, ?2, ?3)",
        params![id, format!("banned: {reason}"), now],
    )?;
    audit(
        &tx,
        actor,
        "ban",
        &format!("account:{id}"),
        json!({ "reason": reason, "until_ms": until }),
        now,
    )?;
    tx.commit()?;
    Ok(until)
}

/// Lift a ban.
pub fn unban(conn: &mut Connection, actor: &str, id: AccountId, now: UnixMs) -> ApiResult<()> {
    let tx = conn.transaction()?;
    require_account(&tx, id)?;
    tx.execute(
        "UPDATE accounts SET banned_until_ms = NULL, ban_reason = NULL WHERE id = ?1",
        params![id],
    )?;
    audit(
        &tx,
        actor,
        "unban",
        &format!("account:{id}"),
        json!({}),
        now,
    )?;
    tx.commit()?;
    Ok(())
}

/// Queue a kick; the hosting instance receives it in its next heartbeat response.
pub fn kick(
    conn: &mut Connection,
    actor: &str,
    id: AccountId,
    reason: &str,
    now: UnixMs,
) -> ApiResult<()> {
    require_reason(reason)?;
    let tx = conn.transaction()?;
    require_account(&tx, id)?;
    tx.execute(
        "INSERT INTO kicks (account_id, reason, created_ms) VALUES (?1, ?2, ?3)",
        params![id, reason, now],
    )?;
    tx.execute(
        "DELETE FROM reconnect_tokens WHERE account_id = ?1",
        params![id],
    )?;
    audit(
        &tx,
        actor,
        "kick",
        &format!("account:{id}"),
        json!({ "reason": reason }),
        now,
    )?;
    tx.commit()?;
    Ok(())
}

/// Credit/debit cash through the economy (`EventReason::Adjustment`), committed with the
/// normal sequence check, plus an audit row. Returns the account after the change.
pub fn adjust(
    conn: &mut Connection,
    economy: &Economy,
    actor: &str,
    id: AccountId,
    cash: i64,
    note: &str,
    now: UnixMs,
) -> ApiResult<wp_economy::PlayerAccount> {
    require_reason(note)?;
    if cash == 0 {
        return Err(ApiError::bad_request("cash must be non-zero"));
    }
    let tx = conn.transaction()?;
    let mut acct = store::load(&tx, PlayerId(id))
        .map_err(store::store_error)?
        .ok_or_else(|| ApiError::not_found(format!("economy account {id}")))?;
    let ((), events) = economy
        .adjust(&mut acct, cash, &format!("admin {actor}: {note}"))
        .map_err(|e| ApiError::conflict("economy_refused", e.to_string()))?;
    store::commit_tx(&tx, &acct, &events, now).map_err(store::store_error)?;
    audit(
        &tx,
        actor,
        "economy_adjust",
        &format!("account:{id}"),
        json!({ "cash": cash, "note": note, "cash_after": acct.cash,
                "event_seqs": events.iter().map(|e| e.seq).collect::<Vec<_>>() }),
        now,
    )?;
    tx.commit()?;
    Ok(acct)
}

/// Everything an admin needs to see about an account.
pub fn account_view(conn: &Connection, id: AccountId) -> ApiResult<AdminAccountView> {
    let profile = auth::profile(conn, id)?;
    let (banned_until_ms, ban_reason): (Option<i64>, Option<String>) = conn.query_row(
        "SELECT banned_until_ms, ban_reason FROM accounts WHERE id = ?1",
        params![id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let economy = store::load(conn, PlayerId(id)).map_err(store::store_error)?;
    Ok(AdminAccountView {
        profile,
        banned_until_ms,
        ban_reason,
        economy,
    })
}

fn validate_flag_key(key: &str) -> ApiResult<()> {
    let ok = (1..=64).contains(&key.len())
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.');
    if ok {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "flag key must be 1-64 chars of a-z 0-9 _ .",
        ))
    }
}

/// Set a feature flag.
pub fn set_flag(
    conn: &mut Connection,
    actor: &str,
    key: &str,
    value: &serde_json::Value,
    now: UnixMs,
) -> ApiResult<()> {
    validate_flag_key(key)?;
    let tx = conn.transaction()?;
    let old: Option<String> = tx
        .query_row(
            "SELECT value_json FROM feature_flags WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()?;
    tx.execute(
        "INSERT INTO feature_flags (key, value_json, updated_ms, updated_by) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json,
             updated_ms = excluded.updated_ms, updated_by = excluded.updated_by",
        params![key, value.to_string(), now, actor],
    )?;
    audit(
        &tx,
        actor,
        "flag_set",
        &format!("flag:{key}"),
        json!({ "old": old.and_then(|o| serde_json::from_str::<serde_json::Value>(&o).ok()),
                "new": value }),
        now,
    )?;
    tx.commit()?;
    Ok(())
}

/// Delete a feature flag (consumers fall back to their defaults).
pub fn delete_flag(conn: &mut Connection, actor: &str, key: &str, now: UnixMs) -> ApiResult<()> {
    let tx = conn.transaction()?;
    let n = tx.execute("DELETE FROM feature_flags WHERE key = ?1", params![key])?;
    if n == 0 {
        return Err(ApiError::not_found(format!("flag {key}")));
    }
    audit(
        &tx,
        actor,
        "flag_delete",
        &format!("flag:{key}"),
        json!({}),
        now,
    )?;
    tx.commit()?;
    Ok(())
}

/// Reset the current period of a board.
pub fn reset_board(
    conn: &mut Connection,
    actor: &str,
    board: &str,
    period: Period,
    now: UnixMs,
) -> ApiResult<usize> {
    let tx = conn.transaction()?;
    let n = leaderboards::reset(&tx, board, period, now)?;
    audit(
        &tx,
        actor,
        "leaderboard_reset",
        &format!("board:{board}"),
        json!({ "period": period.key(now), "rows": n }),
        now,
    )?;
    tx.commit()?;
    Ok(n)
}

/// Latest audit rows, newest first.
pub fn audit_log(conn: &Connection, limit: u32) -> ApiResult<Vec<AuditEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, at_ms, actor, action, target, details FROM audit_log ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit.clamp(1, 1000)], |r| {
            Ok((
                r.get::<_, u32>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(id, at_ms, actor, action, target, details)| {
            Ok(AuditEntry {
                id,
                at_ms,
                actor,
                action,
                target,
                details: serde_json::from_str(&details)?,
            })
        })
        .collect()
}

/// Service + instance health.
pub fn health(conn: &Connection, interval_ms: i64, now: UnixMs) -> ApiResult<HealthReport> {
    directory::refresh_liveness(conn, interval_ms, now)?;
    Ok(HealthReport {
        now_ms: now,
        schema_version: crate::db::schema_version(conn)?,
        accounts: conn.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?,
        queued_tickets: conn.query_row(
            "SELECT COUNT(*) FROM tickets WHERE state = 'queued'",
            [],
            |r| r.get(0),
        )?,
        instances: directory::list(conn, true, now)?,
    })
}
