//! Crews (player groups with a tag) and the districts they hold. Territory changes are
//! reported by instances (the city simulation decides who holds a district).

use rusqlite::{params, Connection, OptionalExtension};

use crate::api::{AccountId, CrewInfo, InstanceId};
use crate::clock::UnixMs;
use crate::error::{ApiError, ApiResult};

fn crew_of(conn: &Connection, account: AccountId) -> ApiResult<Option<u32>> {
    Ok(conn
        .query_row(
            "SELECT crew_id FROM crew_members WHERE account_id = ?1",
            params![account],
            |r| r.get(0),
        )
        .optional()?)
}

/// Create a crew led by the caller.
pub fn create(
    conn: &mut Connection,
    account: AccountId,
    name: &str,
    tag: &str,
    now: UnixMs,
) -> ApiResult<CrewInfo> {
    let name = name.trim();
    let tag = tag.trim().to_uppercase();
    if !(3..=32).contains(&name.chars().count()) {
        return Err(ApiError::bad_request("crew name must be 3-32 characters"));
    }
    if !(2..=5).contains(&tag.len()) || !tag.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(ApiError::bad_request(
            "crew tag must be 2-5 letters or digits",
        ));
    }
    let tx = conn.transaction()?;
    if crew_of(&tx, account)?.is_some() {
        return Err(ApiError::conflict(
            "already_in_crew",
            "leave your crew first",
        ));
    }
    let taken = tx
        .query_row(
            "SELECT 1 FROM crews WHERE name = ?1 COLLATE NOCASE OR tag = ?2",
            params![name, tag],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if taken {
        return Err(ApiError::conflict("crew_taken", "crew name or tag taken"));
    }
    tx.execute(
        "INSERT INTO crews (name, tag, leader_id, created_ms) VALUES (?1, ?2, ?3, ?4)",
        params![name, tag, account, now],
    )?;
    let id = u32::try_from(tx.last_insert_rowid()).map_err(|_| ApiError::internal("ids"))?;
    tx.execute(
        "INSERT INTO crew_members (account_id, crew_id, joined_ms) VALUES (?1, ?2, ?3)",
        params![account, id, now],
    )?;
    tx.commit()?;
    get(conn, id)
}

/// Join a crew (open membership for now; invites are a known gap).
pub fn join(conn: &Connection, account: AccountId, crew: u32, now: UnixMs) -> ApiResult<CrewInfo> {
    if crew_of(conn, account)?.is_some() {
        return Err(ApiError::conflict(
            "already_in_crew",
            "leave your crew first",
        ));
    }
    get(conn, crew)?;
    conn.execute(
        "INSERT INTO crew_members (account_id, crew_id, joined_ms) VALUES (?1, ?2, ?3)",
        params![account, crew, now],
    )?;
    get(conn, crew)
}

/// Leave the crew. Leadership passes to the longest-standing member; a crew whose last member
/// leaves is disbanded and its districts become neutral.
pub fn leave(conn: &mut Connection, account: AccountId) -> ApiResult<()> {
    let tx = conn.transaction()?;
    let crew = crew_of(&tx, account)?.ok_or_else(|| ApiError::not_found("not in a crew"))?;
    tx.execute(
        "DELETE FROM crew_members WHERE account_id = ?1",
        params![account],
    )?;
    let next: Option<AccountId> = tx
        .query_row(
            "SELECT account_id FROM crew_members WHERE crew_id = ?1
             ORDER BY joined_ms, account_id LIMIT 1",
            params![crew],
            |r| r.get(0),
        )
        .optional()?;
    match next {
        Some(n) => {
            tx.execute(
                "UPDATE crews SET leader_id = ?2 WHERE id = ?1 AND leader_id = ?3",
                params![crew, n, account],
            )?;
        }
        None => {
            tx.execute(
                "UPDATE territories SET crew_id = NULL WHERE crew_id = ?1",
                params![crew],
            )?;
            tx.execute("DELETE FROM crews WHERE id = ?1", params![crew])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Crew details.
pub fn get(conn: &Connection, crew: u32) -> ApiResult<CrewInfo> {
    let (name, tag, leader_id): (String, String, AccountId) = conn
        .query_row(
            "SELECT name, tag, leader_id FROM crews WHERE id = ?1",
            params![crew],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| ApiError::not_found(format!("crew {crew}")))?;
    let mut stmt = conn.prepare(
        "SELECT account_id FROM crew_members WHERE crew_id = ?1 ORDER BY joined_ms, account_id",
    )?;
    let members = stmt
        .query_map(params![crew], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let mut stmt = conn.prepare(
        "SELECT city, district_id FROM territories WHERE crew_id = ?1 ORDER BY city, district_id",
    )?;
    let territory = stmt
        .query_map(params![crew], |r| {
            Ok(format!(
                "{}/{}",
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?
            ))
        })?
        .collect::<Result<_, _>>()?;
    Ok(CrewInfo {
        crew_id: crew,
        name,
        tag,
        leader_id,
        members,
        territory,
    })
}

/// An instance reports who holds a district of its city.
pub fn set_territory(
    conn: &Connection,
    instance: InstanceId,
    district: &str,
    crew: Option<u32>,
    now: UnixMs,
) -> ApiResult<()> {
    if district.is_empty() || district.len() > 64 {
        return Err(ApiError::bad_request("district_id must be 1-64 characters"));
    }
    if let Some(c) = crew {
        get(conn, c)?;
    }
    let city: String = conn.query_row(
        "SELECT city FROM instances WHERE id = ?1",
        params![instance],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO territories (city, district_id, crew_id, since_ms) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(city, district_id) DO UPDATE SET
             crew_id = excluded.crew_id,
             since_ms = CASE WHEN crew_id IS excluded.crew_id THEN since_ms ELSE excluded.since_ms END",
        params![city, district, crew, now],
    )?;
    Ok(())
}
