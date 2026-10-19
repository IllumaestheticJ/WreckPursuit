//! Results that instances report: races and pursuits. Both are archived append-only and are
//! idempotent by `report_key` (a retried report is acknowledged, not double-counted). They feed
//! the leaderboards; money/rep/bounty changes go through the economy commit, not here.

use std::collections::BTreeSet;

use rusqlite::{params, Connection, OptionalExtension};

use crate::api::{
    self, AccountId, InstanceId, PursuitOutcome, PursuitReport, PursuitReportResponse, RaceReport,
    RaceReportResponse, RatingChange,
};
use crate::clock::UnixMs;
use crate::directory;
use crate::error::{ApiError, ApiResult};
use crate::leaderboards::{self, Combine, Placement, INITIAL_RATING};

/// Most entries in one race report.
pub const MAX_ENTRIES: usize = 64;

/// 403 unless `instance` is the last instance `account` joined through matchmaking. Instances
/// may only report on and read/write the economy of players they host (or hosted last, so a
/// result sent just after the player left still lands); once the player joins elsewhere, the
/// old instance loses access.
pub fn ensure_hosted(conn: &Connection, instance: InstanceId, account: AccountId) -> ApiResult<()> {
    let last: Option<InstanceId> = conn
        .query_row(
            "SELECT instance_id FROM tickets WHERE account_id = ?1 AND state = 'joined'
             ORDER BY id DESC LIMIT 1",
            params![account],
            |r| r.get(0),
        )
        .optional()?;
    if last == Some(instance) {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "not_hosted",
            format!("account {account} is not hosted by instance {instance}"),
        ))
    }
}

fn validate_key(key: &str) -> ApiResult<()> {
    if key.is_empty() || key.len() > 128 {
        Err(ApiError::bad_request("report_key must be 1-128 characters"))
    } else {
        Ok(())
    }
}

fn rating(conn: &Connection, account: AccountId) -> ApiResult<f64> {
    Ok(conn
        .query_row(
            "SELECT clean_rating FROM ratings WHERE account_id = ?1",
            params![account],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(INITIAL_RATING))
}

/// Archive a race and update clean ratings and time-attack boards.
pub fn report_race(
    conn: &mut Connection,
    instance: InstanceId,
    report: &RaceReport,
    now: UnixMs,
) -> ApiResult<RaceReportResponse> {
    validate_key(&report.report_key)?;
    leaderboards::validate_route(&report.route_id)?;
    directory::validate_bracket(&report.bracket)?;
    if report.race_type.is_empty() || report.race_type.len() > 32 {
        return Err(ApiError::bad_request("race_type must be 1-32 characters"));
    }
    if report.entries.is_empty() || report.entries.len() > MAX_ENTRIES {
        return Err(ApiError::bad_request(format!(
            "a race has 1-{MAX_ENTRIES} entries"
        )));
    }
    let mut seen = BTreeSet::new();
    for e in &report.entries {
        if !seen.insert(e.account_id) {
            return Err(ApiError::bad_request("duplicate account in entries"));
        }
        if (e.position == 0) != e.finish_time_ms.is_none() {
            return Err(ApiError::bad_request(
                "finish_time_ms is required for finishers and absent for DNFs",
            ));
        }
    }

    let tx = conn.transaction()?;
    if let Some(race_id) = tx
        .query_row(
            "SELECT id FROM race_events WHERE report_key = ?1",
            params![report.report_key],
            |r| r.get::<_, u32>(0),
        )
        .optional()?
    {
        let mut stmt = tx.prepare(
            "SELECT account_id, rating_before, rating_after FROM race_results
             WHERE race_id = ?1 ORDER BY account_id",
        )?;
        let ratings = stmt
            .query_map(params![race_id], |r| {
                Ok(RatingChange {
                    account_id: r.get(0)?,
                    before: r.get(1)?,
                    after: r.get(2)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        return Ok(RaceReportResponse {
            race_id,
            duplicate: true,
            ratings,
        });
    }
    for e in &report.entries {
        ensure_hosted(&tx, instance, e.account_id)?;
    }

    tx.execute(
        "INSERT INTO race_events (report_key, instance_id, route_id, race_type, mode, bracket,
                                  recorded_ms, json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            report.report_key,
            instance,
            report.route_id,
            report.race_type,
            api::mode_str(report.mode),
            report.bracket,
            now,
            serde_json::to_string(report)?
        ],
    )?;
    let race_id =
        u32::try_from(tx.last_insert_rowid()).map_err(|_| ApiError::internal("race ids"))?;

    let time_attack = report.race_type == "time_attack";
    let mut field = Vec::with_capacity(report.entries.len());
    for e in &report.entries {
        field.push(Placement {
            account: e.account_id,
            rating: rating(&tx, e.account_id)?,
            rank_key: leaderboards::rank_key(e.position, e.clean),
        });
    }
    // Time attack is solo against the clock: no rating change.
    let deltas = if time_attack {
        vec![0.0; field.len()]
    } else {
        leaderboards::rating_changes(&field)
    };

    let mut ratings = Vec::with_capacity(field.len());
    for ((e, p), d) in report.entries.iter().zip(&field).zip(&deltas) {
        let after = p.rating + d;
        tx.execute(
            "INSERT INTO race_results (race_id, account_id, position, finish_time_ms, clean,
                                       rating_before, rating_after)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                race_id,
                e.account_id,
                e.position,
                e.finish_time_ms.map(|t| t as i64),
                e.clean,
                p.rating,
                after
            ],
        )?;
        if !time_attack && field.len() >= 2 {
            tx.execute(
                "INSERT INTO ratings (account_id, clean_rating, races) VALUES (?1, ?2, 1)
                 ON CONFLICT(account_id) DO UPDATE SET
                     clean_rating = excluded.clean_rating, races = races + 1",
                params![e.account_id, after],
            )?;
            leaderboards::record_rating(&tx, e.account_id, after, *d, now)?;
        }
        if time_attack && e.clean {
            if let Some(t) = e.finish_time_ms {
                leaderboards::record(
                    &tx,
                    &format!("time_attack:{}", report.route_id),
                    e.account_id,
                    t as f64,
                    Combine::Min,
                    now,
                )?;
            }
        }
        ratings.push(RatingChange {
            account_id: e.account_id,
            before: p.rating,
            after,
        });
    }
    tx.commit()?;
    ratings.sort_by_key(|r| r.account_id);
    Ok(RaceReportResponse {
        race_id,
        duplicate: false,
        ratings,
    })
}

/// Archive a pursuit outcome and update the bounty and captures boards.
pub fn report_pursuit(
    conn: &mut Connection,
    instance: InstanceId,
    report: &PursuitReport,
    now: UnixMs,
) -> ApiResult<PursuitReportResponse> {
    validate_key(&report.report_key)?;
    if report.max_wanted > 5 {
        return Err(ApiError::bad_request("max_wanted is 0..=5"));
    }
    if report.outcome == PursuitOutcome::Escaped && report.captured_by.is_some() {
        return Err(ApiError::bad_request("an escape has no captured_by"));
    }
    let tx = conn.transaction()?;
    if let Some(id) = tx
        .query_row(
            "SELECT id FROM pursuit_events WHERE report_key = ?1",
            params![report.report_key],
            |r| r.get::<_, u32>(0),
        )
        .optional()?
    {
        return Ok(PursuitReportResponse {
            pursuit_id: id,
            duplicate: true,
        });
    }
    ensure_hosted(&tx, instance, report.racer_id)?;
    for cop in report.captured_by.iter().chain(&report.assists) {
        ensure_hosted(&tx, instance, *cop)?;
    }
    let outcome = match report.outcome {
        PursuitOutcome::Escaped => "escaped",
        PursuitOutcome::Captured => "captured",
    };
    tx.execute(
        "INSERT INTO pursuit_events (report_key, instance_id, racer_id, max_wanted, bounty,
                                     outcome, captured_by, recorded_ms, json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            report.report_key,
            instance,
            report.racer_id,
            report.max_wanted,
            report.bounty,
            outcome,
            report.captured_by,
            now,
            serde_json::to_string(report)?
        ],
    )?;
    let pursuit_id =
        u32::try_from(tx.last_insert_rowid()).map_err(|_| ApiError::internal("pursuit ids"))?;
    leaderboards::record(
        &tx,
        "bounty",
        report.racer_id,
        report.bounty as f64,
        Combine::Max,
        now,
    )?;
    if let Some(cop) = report.captured_by {
        leaderboards::record(&tx, "captures", cop, 1.0, Combine::Add, now)?;
    }
    tx.commit()?;
    Ok(PursuitReportResponse {
        pursuit_id,
        duplicate: false,
    })
}
