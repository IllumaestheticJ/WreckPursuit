//! Matchmaking: players (or whole parties) queue with [`MatchPrefs`]; the matcher reserves
//! seats on a live instance that fits; the player polls for a one-time join token, presents it
//! to the instance, and the instance verifies it here and gets a reconnect token back.
//!
//! Matching rule (deterministic, run after every queue/poll/heartbeat):
//! - tickets are served oldest first; a party is placed all-or-nothing on one instance;
//! - an instance fits when it is `up`, mode and region match, the city matches if requested,
//!   its bracket equals the requested one or is `open`, and the role has enough free seats
//!   (`slots - population - reserved`);
//! - among fitting instances the fullest wins (fill servers before spreading), ties → lowest id.

use rusqlite::{params, Connection, OptionalExtension};
use wp_contracts::{DamageMode, Role};

use crate::api::{
    self, AccountId, Assignment, InstanceId, InstanceState, MatchPrefs, PartyCreated, PartyInfo,
    RoleCounts, TicketState, TicketStatus, VerifyJoinResponse,
};
use crate::auth;
use crate::clock::UnixMs;
use crate::config::Config;
use crate::directory;
use crate::error::{ApiError, ApiResult};
use crate::token;

/// Largest party.
pub const MAX_PARTY_SIZE: usize = 4;

fn flag_bool(conn: &Connection, key: &str, default: bool) -> ApiResult<bool> {
    Ok(directory::flags(conn)?
        .get(key)
        .and_then(|v| v.as_bool())
        .unwrap_or(default))
}

/// Party of an account, if any.
pub fn party_of(conn: &Connection, account: AccountId) -> ApiResult<Option<u32>> {
    Ok(conn
        .query_row(
            "SELECT party_id FROM party_members WHERE account_id = ?1",
            params![account],
            |r| r.get(0),
        )
        .optional()?)
}

fn party_members(conn: &Connection, party: u32) -> ApiResult<Vec<AccountId>> {
    let mut stmt = conn.prepare(
        "SELECT account_id FROM party_members WHERE party_id = ?1 ORDER BY joined_ms, account_id",
    )?;
    let v = stmt
        .query_map(params![party], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(v)
}

fn has_active_ticket(conn: &Connection, account: AccountId) -> ApiResult<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM tickets WHERE account_id = ?1 AND state IN ('queued', 'assigned')",
            params![account],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Put the caller (or the caller's whole party, if they lead one) in the queue.
pub fn queue(
    conn: &mut Connection,
    account: AccountId,
    prefs: &MatchPrefs,
    cfg: &Config,
    now: UnixMs,
) -> ApiResult<TicketStatus> {
    directory::validate_bracket(&prefs.bracket)?;
    if prefs.region.is_empty() || prefs.region.len() > 32 {
        return Err(ApiError::bad_request("region must be 1-32 characters"));
    }
    let police = matches!(prefs.role, Role::PoliceChase | Role::PoliceCaptain);
    if police && !flag_bool(conn, "player_police_enabled", true)? {
        return Err(ApiError::forbidden(
            "feature_disabled",
            "player police are disabled",
        ));
    }
    if prefs.mode == DamageMode::Simulation && !flag_bool(conn, "simulation_mode_enabled", true)? {
        return Err(ApiError::forbidden(
            "feature_disabled",
            "Simulation mode is disabled",
        ));
    }

    let tx = conn.transaction()?;
    let party = party_of(&tx, account)?;
    let members = match party {
        Some(p) => {
            let leader: AccountId = tx.query_row(
                "SELECT leader_id FROM parties WHERE id = ?1",
                params![p],
                |r| r.get(0),
            )?;
            if leader != account {
                return Err(ApiError::forbidden(
                    "not_party_leader",
                    "only the party leader can queue the party",
                ));
            }
            party_members(&tx, p)?
        }
        None => vec![account],
    };
    if prefs.role == Role::PoliceCaptain && members.len() > 1 {
        return Err(ApiError::bad_request(
            "an instance has one captain seat; queue the captain without a party",
        ));
    }
    for m in &members {
        auth::check_not_banned(&tx, *m, now)?;
        if has_active_ticket(&tx, *m)? {
            return Err(ApiError::conflict(
                "already_queued",
                format!("account {m} already has an active ticket"),
            ));
        }
    }
    for m in &members {
        tx.execute(
            "INSERT INTO tickets (account_id, party_id, role, mode, bracket, region, city, state, created_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'queued', ?8)",
            params![
                m,
                party,
                api::role_str(prefs.role),
                api::mode_str(prefs.mode),
                prefs.bracket,
                prefs.region,
                prefs.city,
                now
            ],
        )?;
    }
    tx.commit()?;
    process(conn, cfg, now)?;
    status(conn, account, cfg, now)
}

/// Leave the queue (cancels the whole party's waiting tickets).
pub fn cancel(conn: &Connection, account: AccountId) -> ApiResult<()> {
    let party: Option<Option<u32>> = conn
        .query_row(
            "SELECT party_id FROM tickets
             WHERE account_id = ?1 AND state IN ('queued', 'assigned') ORDER BY id DESC LIMIT 1",
            params![account],
            |r| r.get(0),
        )
        .optional()?;
    match party {
        None => Err(ApiError::not_found("no active ticket")),
        Some(Some(p)) => {
            conn.execute(
                "UPDATE tickets SET state = 'cancelled', join_token_hash = NULL
                 WHERE party_id = ?1 AND state IN ('queued', 'assigned')",
                params![p],
            )?;
            Ok(())
        }
        Some(None) => {
            conn.execute(
                "UPDATE tickets SET state = 'cancelled', join_token_hash = NULL
                 WHERE account_id = ?1 AND state IN ('queued', 'assigned')",
                params![account],
            )?;
            Ok(())
        }
    }
}

struct Candidate {
    id: InstanceId,
    region: String,
    city: String,
    mode: DamageMode,
    bracket: String,
    free: RoleCounts,
    load: u32,
}

struct Group {
    tickets: Vec<i64>,
    role: Role,
    mode: DamageMode,
    bracket: String,
    region: String,
    city: Option<String>,
}

/// Expire stale tickets and assign queued tickets to instances. Idempotent.
pub fn process(conn: &mut Connection, cfg: &Config, now: UnixMs) -> ApiResult<()> {
    let tx = conn.transaction()?;
    directory::refresh_liveness(&tx, cfg.heartbeat_interval_ms, now)?;
    tx.execute(
        "UPDATE tickets SET state = 'expired' WHERE state = 'queued' AND created_ms < ?1",
        params![now - cfg.queue_ttl_ms],
    )?;
    // A seat reserved for someone who never joined is released after two join-token lifetimes
    // without a join.
    tx.execute(
        "UPDATE tickets SET state = 'expired', join_token_hash = NULL
         WHERE state = 'assigned' AND assigned_ms < ?1",
        params![now - 2 * cfg.join_ttl_ms],
    )?;

    // Joined players the instance never lists in a heartbeat stop holding a seat after a while
    // (the population it reports is then authoritative).
    tx.execute(
        "UPDATE tickets SET reported = 1
         WHERE state = 'joined' AND reported = 0 AND joined_ms < ?1",
        params![now - directory::UNREPORTED_JOIN_HOLD_MS],
    )?;

    let mut candidates = Vec::new();
    for info in directory::list(&tx, false, now)? {
        if info.state != InstanceState::Up {
            continue;
        }
        let mut free = RoleCounts::default();
        for role in directory::ALL_ROLES {
            *free.get_mut(role) = info
                .slots
                .get(role)
                .saturating_sub(info.population.get(role))
                .saturating_sub(info.reserved.get(role));
        }
        candidates.push(Candidate {
            id: info.instance_id,
            region: info.region,
            city: info.city,
            mode: info.mode,
            bracket: info.bracket,
            free,
            load: info.population.total() + info.reserved.total(),
        });
    }

    let groups = queued_groups(&tx)?;
    for g in groups {
        let size = g.tickets.len() as u32;
        let best = candidates
            .iter_mut()
            .filter(|c| {
                c.mode == g.mode
                    && c.region == g.region
                    && g.city.as_ref().is_none_or(|city| *city == c.city)
                    && (c.bracket == g.bracket || c.bracket == "open")
                    && c.free.get(g.role) >= size
            })
            // max_by_key keeps the last maximum; iterate in reverse id order so ties go to the
            // lowest id.
            .rev()
            .max_by_key(|c| c.load);
        let Some(inst) = best else { continue };
        *inst.free.get_mut(g.role) -= size;
        inst.load += size;
        for t in &g.tickets {
            tx.execute(
                "UPDATE tickets SET state = 'assigned', instance_id = ?2, assigned_ms = ?3
                 WHERE id = ?1",
                params![t, inst.id, now],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Queued tickets grouped by party (singles are their own group), oldest first.
fn queued_groups(conn: &Connection) -> ApiResult<Vec<Group>> {
    let mut stmt = conn.prepare(
        "SELECT id, party_id, role, mode, bracket, region, city FROM tickets
         WHERE state = 'queued' ORDER BY id",
    )?;
    type Raw = (
        i64,
        Option<u32>,
        String,
        String,
        String,
        String,
        Option<String>,
    );
    let rows: Vec<Raw> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })?
        .collect::<Result<_, _>>()?;
    let mut groups: Vec<(Option<u32>, Group)> = Vec::new();
    for (id, party, role, mode, bracket, region, city) in rows {
        if let Some(p) = party {
            if let Some((_, g)) = groups.iter_mut().find(|(gp, _)| *gp == Some(p)) {
                g.tickets.push(id);
                continue;
            }
        }
        let (Some(role), Some(mode)) = (api::parse_role(&role), api::parse_mode(&mode)) else {
            continue;
        };
        groups.push((
            party,
            Group {
                tickets: vec![id],
                role,
                mode,
                bracket,
                region,
                city,
            },
        ));
    }
    Ok(groups.into_iter().map(|(_, g)| g).collect())
}

/// The caller's latest ticket. When assigned, a fresh join token is minted (older ones stop
/// working).
pub fn status(
    conn: &Connection,
    account: AccountId,
    cfg: &Config,
    now: UnixMs,
) -> ApiResult<TicketStatus> {
    type Row = (
        u32,
        String,
        Option<u32>,
        Option<InstanceId>,
        String,
        Option<i64>,
    );
    let row: Option<Row> = conn
        .query_row(
            "SELECT id, state, party_id, instance_id, role, assigned_ms FROM tickets
             WHERE account_id = ?1 ORDER BY id DESC LIMIT 1",
            params![account],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .optional()?;
    let (ticket_id, state, party_id, instance_id, role, assigned_ms) =
        row.ok_or_else(|| ApiError::not_found("no ticket"))?;
    let state = TicketState::parse(&state).ok_or_else(|| ApiError::internal("bad ticket state"))?;
    let mut assignment = None;
    if let (TicketState::Assigned, Some(inst)) = (state, instance_id) {
        let join_token = token::generate(token::prefix::JOIN);
        // Never valid past the point where `process` releases the reserved seat.
        let seat_released_ms = assigned_ms.unwrap_or(now) + 2 * cfg.join_ttl_ms;
        let join_expires_ms = (now + cfg.join_ttl_ms).min(seat_released_ms);
        conn.execute(
            "UPDATE tickets SET join_token_hash = ?2, join_expires_ms = ?3 WHERE id = ?1",
            params![ticket_id, token::hash(&join_token), join_expires_ms],
        )?;
        let address: String = conn.query_row(
            "SELECT address FROM instances WHERE id = ?1",
            params![inst],
            |r| r.get(0),
        )?;
        assignment = Some(Assignment {
            instance_id: inst,
            address,
            join_token,
            join_expires_ms,
            role: api::parse_role(&role).unwrap_or(Role::Spectator),
        });
    }
    Ok(TicketStatus {
        ticket_id,
        state,
        party_id,
        assignment,
    })
}

/// An instance checks a join token a client presented. One-time: the ticket becomes `joined`.
pub fn verify_join(
    conn: &Connection,
    instance: InstanceId,
    join_token: &str,
    cfg: &Config,
    now: UnixMs,
) -> ApiResult<VerifyJoinResponse> {
    let row: Option<(i64, AccountId, String, Option<u32>)> = conn
        .query_row(
            "SELECT id, account_id, role, party_id FROM tickets
             WHERE join_token_hash = ?1 AND state = 'assigned' AND instance_id = ?2
               AND join_expires_ms > ?3",
            params![token::hash(join_token), instance, now],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let (ticket, account_id, role, party_id) = row.ok_or_else(|| {
        ApiError::new(
            axum::http::StatusCode::UNAUTHORIZED,
            "invalid_join_token",
            "join token unknown, expired, used or for another instance",
        )
    })?;
    auth::check_not_banned(conn, account_id, now)?;
    conn.execute(
        "UPDATE tickets SET state = 'joined', join_token_hash = NULL, joined_ms = ?2, reported = 0
         WHERE id = ?1",
        params![ticket, now],
    )?;
    let reconnect_token = token::generate(token::prefix::RECONNECT);
    let reconnect_expires_ms = now + cfg.reconnect_ttl_ms;
    conn.execute(
        "DELETE FROM reconnect_tokens WHERE account_id = ?1",
        params![account_id],
    )?;
    conn.execute(
        "INSERT INTO reconnect_tokens (token_hash, account_id, instance_id, role, expires_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            token::hash(&reconnect_token),
            account_id,
            instance,
            role,
            reconnect_expires_ms
        ],
    )?;
    let display_name: String = conn.query_row(
        "SELECT display_name FROM accounts WHERE id = ?1",
        params![account_id],
        |r| r.get(0),
    )?;
    Ok(VerifyJoinResponse {
        account_id,
        display_name,
        role: api::parse_role(&role).unwrap_or(Role::Spectator),
        party_id,
        reconnect_token,
        reconnect_expires_ms,
    })
}

/// Rejoin the instance a reconnect token was issued for (bypasses the queue; the seat is
/// still the player's). Returns an assigned ticket with a fresh join token.
pub fn reconnect(
    conn: &mut Connection,
    account: AccountId,
    reconnect_token: &str,
    cfg: &Config,
    now: UnixMs,
) -> ApiResult<TicketStatus> {
    let tx = conn.transaction()?;
    let row: Option<(InstanceId, String)> = tx
        .query_row(
            "SELECT r.instance_id, r.role FROM reconnect_tokens r
             JOIN instances i ON i.id = r.instance_id
             WHERE r.token_hash = ?1 AND r.account_id = ?2 AND r.expires_ms > ?3
               AND i.state != 'down'",
            params![token::hash(reconnect_token), account, now],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (instance, role) = row.ok_or_else(|| {
        ApiError::new(
            axum::http::StatusCode::GONE,
            "reconnect_unavailable",
            "reconnect token unknown or expired, or the instance is gone; queue again",
        )
    })?;
    tx.execute(
        "UPDATE tickets SET state = 'cancelled', join_token_hash = NULL
         WHERE account_id = ?1 AND state IN ('queued', 'assigned')",
        params![account],
    )?;
    let party = party_of(&tx, account)?;
    let (mode, region, city, bracket): (String, String, String, String) = tx.query_row(
        "SELECT mode, region, city, bracket FROM instances WHERE id = ?1",
        params![instance],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    tx.execute(
        "INSERT INTO tickets (account_id, party_id, role, mode, bracket, region, city, state,
                              instance_id, created_ms, assigned_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'assigned', ?8, ?9, ?9)",
        params![account, party, role, mode, bracket, region, city, instance, now],
    )?;
    tx.execute(
        "DELETE FROM reconnect_tokens WHERE token_hash = ?1",
        params![token::hash(reconnect_token)],
    )?;
    tx.commit()?;
    status(conn, account, cfg, now)
}

/// Create a party led by the caller.
pub fn create_party(
    conn: &mut Connection,
    account: AccountId,
    now: UnixMs,
) -> ApiResult<PartyCreated> {
    let tx = conn.transaction()?;
    if party_of(&tx, account)?.is_some() {
        return Err(ApiError::conflict(
            "already_in_party",
            "leave your party first",
        ));
    }
    let code = token::short_code();
    tx.execute(
        "INSERT INTO parties (leader_id, code_hash, created_ms) VALUES (?1, ?2, ?3)",
        params![account, token::hash(&code), now],
    )?;
    let party_id = u32::try_from(tx.last_insert_rowid()).map_err(|_| ApiError::internal("ids"))?;
    tx.execute(
        "INSERT INTO party_members (account_id, party_id, joined_ms) VALUES (?1, ?2, ?3)",
        params![account, party_id, now],
    )?;
    tx.commit()?;
    Ok(PartyCreated { party_id, code })
}

/// Join a party with its invite code.
pub fn join_party(
    conn: &mut Connection,
    account: AccountId,
    party_id: u32,
    code: &str,
    now: UnixMs,
) -> ApiResult<PartyInfo> {
    let tx = conn.transaction()?;
    if party_of(&tx, account)?.is_some() {
        return Err(ApiError::conflict(
            "already_in_party",
            "leave your party first",
        ));
    }
    let ok = tx
        .query_row(
            "SELECT 1 FROM parties WHERE id = ?1 AND code_hash = ?2",
            params![party_id, token::hash(&code.trim().to_uppercase())],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !ok {
        return Err(ApiError::forbidden(
            "bad_party_code",
            "unknown party or wrong code",
        ));
    }
    let members = party_members(&tx, party_id)?;
    if members.len() >= MAX_PARTY_SIZE {
        return Err(ApiError::conflict("party_full", "party is full"));
    }
    for m in &members {
        if has_active_ticket(&tx, *m)? {
            return Err(ApiError::conflict(
                "party_in_queue",
                "the party is queued; ask the leader to leave the queue first",
            ));
        }
    }
    tx.execute(
        "INSERT INTO party_members (account_id, party_id, joined_ms) VALUES (?1, ?2, ?3)",
        params![account, party_id, now],
    )?;
    tx.commit()?;
    party(conn, account, party_id)
}

/// Leave the current party. Waiting party tickets are cancelled; leadership passes to the
/// longest-standing member; an empty party is deleted.
pub fn leave_party(conn: &mut Connection, account: AccountId) -> ApiResult<()> {
    let tx = conn.transaction()?;
    let p = party_of(&tx, account)?.ok_or_else(|| ApiError::not_found("not in a party"))?;
    tx.execute(
        "UPDATE tickets SET state = 'cancelled', join_token_hash = NULL
         WHERE party_id = ?1 AND state IN ('queued', 'assigned')",
        params![p],
    )?;
    tx.execute(
        "DELETE FROM party_members WHERE account_id = ?1",
        params![account],
    )?;
    let rest = party_members(&tx, p)?;
    match rest.first() {
        None => {
            tx.execute("DELETE FROM parties WHERE id = ?1", params![p])?;
        }
        Some(next) => {
            tx.execute(
                "UPDATE parties SET leader_id = ?2 WHERE id = ?1 AND leader_id = ?3",
                params![p, next, account],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Party details (members only).
pub fn party(conn: &Connection, account: AccountId, party_id: u32) -> ApiResult<PartyInfo> {
    if party_of(conn, account)? != Some(party_id) {
        return Err(ApiError::forbidden(
            "not_a_member",
            "not a member of this party",
        ));
    }
    let leader_id: AccountId = conn.query_row(
        "SELECT leader_id FROM parties WHERE id = ?1",
        params![party_id],
        |r| r.get(0),
    )?;
    Ok(PartyInfo {
        party_id,
        leader_id,
        members: party_members(conn, party_id)?,
    })
}
