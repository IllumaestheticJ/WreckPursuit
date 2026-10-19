//! Instance directory: city servers register, heartbeat their population, receive kicks and
//! feature flags, and are marked down after three missed heartbeats.

use std::collections::BTreeMap;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::api::{
    self, HeartbeatRequest, HeartbeatResponse, InstanceId, InstanceInfo, InstanceState, KickOrder,
    RegisterInstanceRequest, RegisterInstanceResponse, RoleCounts,
};
use crate::clock::UnixMs;
use crate::error::{ApiError, ApiResult};
use crate::token;

/// Heartbeats an instance may miss before it is marked down.
pub const MISSED_HEARTBEATS_DOWN: i64 = 3;
/// Undelivered kicks older than this are dropped (the player is gone; bans block rejoin).
pub const KICK_TTL_MS: i64 = 3_600_000;

/// How long a joined player who never appears in a heartbeat's `players` list keeps holding
/// a seat. Longer than several heartbeat intervals.
pub const UNREPORTED_JOIN_HOLD_MS: i64 = 120_000;

/// Valid bracket names (`open` accepts every bracket).
pub const BRACKETS: &[&str] = &["D", "C", "B", "A", "S", "open"];

/// 400 unless `b` is a known bracket.
pub fn validate_bracket(b: &str) -> ApiResult<()> {
    if BRACKETS.contains(&b) {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "unknown bracket '{b}' (expected one of {BRACKETS:?})"
        )))
    }
}

fn text_ok(field: &str, v: &str, max: usize) -> ApiResult<()> {
    if v.is_empty() || v.len() > max {
        Err(ApiError::bad_request(format!(
            "{field} must be 1-{max} characters"
        )))
    } else {
        Ok(())
    }
}

/// Register a new instance. The caller has already checked the registration key.
pub fn register(
    conn: &Connection,
    req: &RegisterInstanceRequest,
    heartbeat_interval_ms: i64,
    now: UnixMs,
) -> ApiResult<RegisterInstanceResponse> {
    text_ok("name", &req.name, 64)?;
    text_ok("region", &req.region, 32)?;
    text_ok("city", &req.city, 64)?;
    text_ok("address", &req.address, 256)?;
    validate_bracket(&req.bracket)?;
    if req.slots.police_captain > 1 {
        return Err(ApiError::bad_request(
            "at most one police captain per instance",
        ));
    }
    if req.slots.total() == 0 {
        return Err(ApiError::bad_request("instance has no slots"));
    }
    let instance_token = token::generate(token::prefix::INSTANCE);
    conn.execute(
        "INSERT INTO instances (token_hash, name, region, city, mode, bracket, address,
                                slots_json, population_json, state, registered_ms, last_heartbeat_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'up', ?10, ?10)",
        params![
            token::hash(&instance_token),
            req.name,
            req.region,
            req.city,
            api::mode_str(req.mode),
            req.bracket,
            req.address,
            serde_json::to_string(&req.slots)?,
            serde_json::to_string(&RoleCounts::default())?,
            now
        ],
    )?;
    let instance_id = InstanceId::try_from(conn.last_insert_rowid())
        .map_err(|_| ApiError::internal("instance ids exhausted"))?;
    Ok(RegisterInstanceResponse {
        instance_id,
        instance_token,
        heartbeat_interval_ms,
    })
}

/// Check an instance bearer token against the instance id in the path.
pub fn authenticate(conn: &Connection, id: InstanceId, token_str: &str) -> ApiResult<()> {
    let ok = conn
        .query_row(
            "SELECT 1 FROM instances WHERE id = ?1 AND token_hash = ?2",
            params![id, token::hash(token_str)],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if ok {
        Ok(())
    } else {
        Err(ApiError::unauthorized())
    }
}

/// Mark instances that stopped heartbeating as down and put their unclaimed seats back in the
/// queue.
pub fn refresh_liveness(conn: &Connection, interval_ms: i64, now: UnixMs) -> ApiResult<()> {
    let cutoff = now - MISSED_HEARTBEATS_DOWN * interval_ms;
    conn.execute(
        "UPDATE instances SET state = 'down' WHERE state != 'down' AND last_heartbeat_ms < ?1",
        params![cutoff],
    )?;
    conn.execute(
        "UPDATE tickets SET state = 'queued', instance_id = NULL, join_token_hash = NULL,
                            join_expires_ms = NULL, assigned_ms = NULL
         WHERE state = 'assigned'
           AND instance_id IN (SELECT id FROM instances WHERE state = 'down')",
        [],
    )?;
    Ok(())
}

/// Record a heartbeat; returns pending kicks and the feature flags.
pub fn heartbeat(
    conn: &Connection,
    id: InstanceId,
    req: &HeartbeatRequest,
    reconnect_ttl_ms: i64,
    now: UnixMs,
) -> ApiResult<HeartbeatResponse> {
    let slots = instance(conn, id)?.slots;
    for role in ALL_ROLES {
        if req.population.get(role) > slots.get(role) {
            return Err(ApiError::bad_request(format!(
                "population {:?} exceeds slots for {:?}",
                req.population, role
            )));
        }
    }
    let state = if req.draining {
        InstanceState::Draining
    } else {
        InstanceState::Up
    };
    conn.execute(
        "UPDATE instances SET population_json = ?2, state = ?3, last_heartbeat_ms = ?4,
                              tick_ms = ?5
         WHERE id = ?1",
        params![
            id,
            serde_json::to_string(&req.population)?,
            state.as_str(),
            now,
            req.tick_ms as f64
        ],
    )?;
    for p in &req.players {
        // The reported population now covers this player: release the join hold.
        conn.execute(
            "UPDATE tickets SET reported = 1
             WHERE account_id = ?1 AND instance_id = ?2 AND state = 'joined' AND reported = 0",
            params![p, id],
        )?;
        conn.execute(
            "UPDATE reconnect_tokens SET expires_ms = ?3
             WHERE account_id = ?1 AND instance_id = ?2",
            params![p, id, now + reconnect_ttl_ms],
        )?;
    }
    let (sum, n) = listed_racer_ratings(conn, id, &req.players)?;
    conn.execute(
        "UPDATE instances SET racer_rating_sum = ?2, racer_rated = ?3 WHERE id = ?1",
        params![id, sum, n],
    )?;
    let kicks = take_kicks(conn, id, &req.players, now)?;
    Ok(HeartbeatResponse {
        kicks,
        flags: flags(conn)?,
    })
}

/// Summed current clean rating and count of the listed players who hold a racer seat here
/// (their latest joined ticket is a racer ticket on this instance).
fn listed_racer_ratings(
    conn: &Connection,
    id: InstanceId,
    players: &[api::AccountId],
) -> ApiResult<(f64, u32)> {
    let mut stmt = conn.prepare(
        "SELECT role, instance_id FROM tickets
         WHERE account_id = ?1 AND state = 'joined' ORDER BY id DESC LIMIT 1",
    )?;
    let (mut sum, mut n) = (0.0, 0u32);
    for p in players {
        let seat: Option<(String, Option<InstanceId>)> = stmt
            .query_row(params![p], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        if matches!(&seat, Some((role, Some(i))) if role == "racer" && *i == id) {
            sum += crate::leaderboards::clean_rating(conn, *p)?;
            n += 1;
        }
    }
    Ok((sum, n))
}

/// Every role, in a fixed order.
pub const ALL_ROLES: [wp_contracts::Role; 4] = [
    wp_contracts::Role::Racer,
    wp_contracts::Role::PoliceChase,
    wp_contracts::Role::PoliceCaptain,
    wp_contracts::Role::Spectator,
];

/// Pending kicks for players on this instance (listed in the heartbeat, or whose latest
/// joined ticket points here). Marks them delivered.
fn take_kicks(
    conn: &Connection,
    id: InstanceId,
    players: &[api::AccountId],
    now: UnixMs,
) -> ApiResult<Vec<KickOrder>> {
    let mut stmt = conn.prepare(
        "SELECT k.id, k.account_id, k.reason FROM kicks k
         WHERE k.delivered_ms IS NULL AND k.created_ms >= ?2
           AND (SELECT t.instance_id FROM tickets t
                WHERE t.account_id = k.account_id AND t.state = 'joined'
                ORDER BY t.id DESC LIMIT 1) = ?1
         ORDER BY k.id",
    )?;
    let mut rows: Vec<(i64, api::AccountId, String)> = stmt
        .query_map(params![id, now - KICK_TTL_MS], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?
        .collect::<Result<_, _>>()?;
    for p in players {
        let mut s = conn.prepare(
            "SELECT id, account_id, reason FROM kicks
             WHERE delivered_ms IS NULL AND created_ms >= ?2 AND account_id = ?1 ORDER BY id",
        )?;
        let more: Vec<(i64, api::AccountId, String)> = s
            .query_map(params![p, now - KICK_TTL_MS], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<Result<_, _>>()?;
        for m in more {
            if !rows.iter().any(|r| r.0 == m.0) {
                rows.push(m);
            }
        }
    }
    let mut out = Vec::with_capacity(rows.len());
    for (kick_id, account_id, reason) in rows {
        conn.execute(
            "UPDATE kicks SET delivered_ms = ?2 WHERE id = ?1",
            params![kick_id, now],
        )?;
        out.push(KickOrder { account_id, reason });
    }
    Ok(out)
}

/// All feature flags.
pub fn flags(conn: &Connection) -> ApiResult<BTreeMap<String, serde_json::Value>> {
    let mut stmt = conn.prepare("SELECT key, value_json FROM feature_flags ORDER BY key")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = BTreeMap::new();
    for r in rows {
        let (k, v) = r?;
        out.insert(k, serde_json::from_str(&v)?);
    }
    Ok(out)
}

/// Deregister: the instance is down and its token stops working.
pub fn deregister(conn: &Connection, id: InstanceId) -> ApiResult<()> {
    conn.execute(
        "UPDATE instances SET state = 'down', token_hash = ?2 WHERE id = ?1",
        params![id, format!("revoked:{id}")],
    )?;
    conn.execute(
        "UPDATE tickets SET state = 'queued', instance_id = NULL, join_token_hash = NULL,
                            join_expires_ms = NULL, assigned_ms = NULL
         WHERE state = 'assigned' AND instance_id = ?1",
        params![id],
    )?;
    conn.execute(
        "DELETE FROM reconnect_tokens WHERE instance_id = ?1",
        params![id],
    )?;
    Ok(())
}

const INSTANCE_COLUMNS: &str = "id, name, region, city, mode, bracket, address, slots_json,
     population_json, state, last_heartbeat_ms, tick_ms, racer_rating_sum, racer_rated";

fn instance_from_row(r: &Row<'_>) -> rusqlite::Result<InstanceInfo> {
    let json_err = |e: serde_json::Error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    };
    let mode: String = r.get(4)?;
    let state: String = r.get(9)?;
    Ok(InstanceInfo {
        instance_id: r.get(0)?,
        name: r.get(1)?,
        region: r.get(2)?,
        city: r.get(3)?,
        mode: api::parse_mode(&mode).unwrap_or_default(),
        bracket: r.get(5)?,
        address: r.get(6)?,
        slots: serde_json::from_str(&r.get::<_, String>(7)?).map_err(json_err)?,
        population: serde_json::from_str(&r.get::<_, String>(8)?).map_err(json_err)?,
        reserved: RoleCounts::default(),
        state: InstanceState::parse(&state).unwrap_or(InstanceState::Down),
        last_heartbeat_ms: r.get(10)?,
        tick_ms: r.get::<_, f64>(11)? as f32,
        racer_rating: {
            let (sum, n): (f64, u32) = (r.get(12)?, r.get(13)?);
            (n > 0).then(|| sum / n as f64)
        },
    })
}

/// Seats held by matched players the instance's reported population does not cover yet:
/// assigned tickets (not connected yet) and joined tickets no heartbeat has listed yet.
pub fn reserved(conn: &Connection, id: InstanceId) -> ApiResult<RoleCounts> {
    Ok(reserved_all(conn)?.remove(&id).unwrap_or_default())
}

/// [`reserved`] for every instance that has any, in one query.
pub fn reserved_all(conn: &Connection) -> ApiResult<BTreeMap<InstanceId, RoleCounts>> {
    let mut stmt = conn.prepare(
        "SELECT instance_id, role, COUNT(*) FROM tickets
         WHERE state = 'assigned' OR (state = 'joined' AND reported = 0)
         GROUP BY instance_id, role",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, InstanceId>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, u32>(2)?,
        ))
    })?;
    let mut out: BTreeMap<InstanceId, RoleCounts> = BTreeMap::new();
    for r in rows {
        let (id, role, n) = r?;
        if let Some(role) = api::parse_role(&role) {
            *out.entry(id).or_default().get_mut(role) = n;
        }
    }
    Ok(out)
}

/// One instance (with reserved seats filled in).
pub fn instance(conn: &Connection, id: InstanceId) -> ApiResult<InstanceInfo> {
    let mut info = conn
        .query_row(
            &format!("SELECT {INSTANCE_COLUMNS} FROM instances WHERE id = ?1"),
            params![id],
            instance_from_row,
        )
        .optional()?
        .ok_or_else(|| ApiError::not_found(format!("instance {id}")))?;
    info.reserved = reserved(conn, id)?;
    Ok(info)
}

/// Instances, ordered by id. `include_down` also lists instances seen in the last day that
/// are down.
pub fn list(conn: &Connection, include_down: bool, now: UnixMs) -> ApiResult<Vec<InstanceInfo>> {
    let sql = if include_down {
        format!(
            "SELECT {INSTANCE_COLUMNS} FROM instances WHERE state != 'down' OR last_heartbeat_ms >= ?1 ORDER BY id"
        )
    } else {
        format!("SELECT {INSTANCE_COLUMNS} FROM instances WHERE state != 'down' AND ?1 = ?1 ORDER BY id")
    };
    let mut stmt = conn.prepare(&sql)?;
    let mut out: Vec<InstanceInfo> = stmt
        .query_map(params![now - 86_400_000], instance_from_row)?
        .collect::<Result<_, _>>()?;
    let mut reserved = reserved_all(conn)?;
    for i in &mut out {
        i.reserved = reserved.remove(&i.instance_id).unwrap_or_default();
    }
    Ok(out)
}
