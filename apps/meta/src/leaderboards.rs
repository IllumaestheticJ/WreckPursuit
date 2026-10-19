//! Leaderboards with all-time, daily and weekly periods.
//!
//! Boards:
//! - `clean_rating` — Elo-style rating from race results where dirty finishes rank behind every
//!   clean finish (see [`rating_changes`]). `all` holds the rating; daily/weekly hold the rating
//!   *gained* in that period.
//! - `bounty` — highest bounty carried at the end of a pursuit in the period.
//! - `captures` — captures credited to a player cop in the period.
//! - `time_attack:<route>` — best clean time-attack time (ms, lower is better).
//!
//! A period is a key (`all`, `d:YYYY-MM-DD`, `w:YYYY-MM-DD` of the Monday, UTC); a new day or
//! week simply starts a new key, which is the reset. Old periods stay queryable.

use rusqlite::{params, Connection};

use crate::api::{AccountId, Leaderboard, LeaderboardRow};
use crate::clock::{daily_period, weekly_period, UnixMs};
use crate::error::{ApiError, ApiResult};

/// Rating of an account with no races.
pub const INITIAL_RATING: f64 = 1500.0;
/// Elo K-factor for a whole race (split over opponents).
pub const RATING_K: f64 = 32.0;
/// Most rows a query returns.
pub const MAX_LIMIT: u32 = 200;

/// Fixed board ids (time-attack boards are `time_attack:<route>`).
pub const BOARDS: &[&str] = &["clean_rating", "bounty", "captures"];

/// Current clean rating of an account ([`INITIAL_RATING`] before its first rated race).
pub fn clean_rating(conn: &Connection, account: AccountId) -> ApiResult<f64> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row(
            "SELECT clean_rating FROM ratings WHERE account_id = ?1",
            params![account],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(INITIAL_RATING))
}

/// Which period of a board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    /// Since launch.
    All,
    /// Current UTC day.
    Daily,
    /// Current week (Monday 00:00 UTC).
    Weekly,
}

impl Period {
    /// Parse `all` / `daily` / `weekly`.
    pub fn parse(s: &str) -> ApiResult<Self> {
        match s {
            "all" => Ok(Self::All),
            "daily" => Ok(Self::Daily),
            "weekly" => Ok(Self::Weekly),
            _ => Err(ApiError::bad_request("period must be all, daily or weekly")),
        }
    }

    /// Storage key of this period at `now`.
    pub fn key(self, now: UnixMs) -> String {
        match self {
            Self::All => "all".into(),
            Self::Daily => daily_period(now),
            Self::Weekly => weekly_period(now),
        }
    }
}

/// Route ids appear in board names: 1-48 chars of `[a-z0-9_-]`.
pub fn validate_route(route: &str) -> ApiResult<()> {
    let ok = (1..=48).contains(&route.len())
        && route
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if ok {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "route_id must be 1-48 chars of a-z 0-9 _ -",
        ))
    }
}

/// 400 unless `board` is a known board or a valid `time_attack:<route>`.
pub fn validate_board(board: &str) -> ApiResult<()> {
    if BOARDS.contains(&board) {
        return Ok(());
    }
    match board.strip_prefix("time_attack:") {
        Some(route) => validate_route(route),
        None => Err(ApiError::not_found(format!("board '{board}'"))),
    }
}

/// Lower is better on this board.
pub fn ascending(board: &str) -> bool {
    board.starts_with("time_attack:")
}

/// How a new value combines with the stored score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combine {
    /// Keep the larger.
    Max,
    /// Keep the smaller.
    Min,
    /// Add.
    Add,
    /// Replace.
    Set,
}

fn upsert(
    conn: &Connection,
    board: &str,
    period: &str,
    account: AccountId,
    value: f64,
    how: Combine,
    now: UnixMs,
) -> ApiResult<()> {
    let update = match how {
        Combine::Max => "MAX(score, excluded.score)",
        Combine::Min => "MIN(score, excluded.score)",
        Combine::Add => "score + excluded.score",
        Combine::Set => "excluded.score",
    };
    conn.execute(
        &format!(
            "INSERT INTO lb_entries (board, period, account_id, score, updated_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(board, period, account_id) DO UPDATE SET
                 score = {update}, updated_ms = excluded.updated_ms"
        ),
        params![board, period, account, value, now],
    )?;
    Ok(())
}

/// Record a value in the all-time, daily and weekly periods of a board.
pub fn record(
    conn: &Connection,
    board: &str,
    account: AccountId,
    value: f64,
    how: Combine,
    now: UnixMs,
) -> ApiResult<()> {
    for p in [Period::All, Period::Daily, Period::Weekly] {
        upsert(conn, board, &p.key(now), account, value, how, now)?;
    }
    Ok(())
}

/// Record a clean-rating change: all-time gets the new rating, daily/weekly the delta.
pub fn record_rating(
    conn: &Connection,
    account: AccountId,
    after: f64,
    delta: f64,
    now: UnixMs,
) -> ApiResult<()> {
    upsert(
        conn,
        "clean_rating",
        "all",
        account,
        after,
        Combine::Set,
        now,
    )?;
    for p in [Period::Daily, Period::Weekly] {
        upsert(
            conn,
            "clean_rating",
            &p.key(now),
            account,
            delta,
            Combine::Add,
            now,
        )?;
    }
    Ok(())
}

/// Top rows of a board for the current period of `period`. Ties: earlier `updated_ms` first,
/// then lower account id.
pub fn top(
    conn: &Connection,
    board: &str,
    period: Period,
    limit: u32,
    now: UnixMs,
) -> ApiResult<Leaderboard> {
    validate_board(board)?;
    let key = period.key(now);
    let asc = ascending(board);
    let order = if asc { "ASC" } else { "DESC" };
    let mut stmt = conn.prepare(&format!(
        "SELECT e.account_id, a.display_name, e.score FROM lb_entries e
         JOIN accounts a ON a.id = e.account_id
         WHERE e.board = ?1 AND e.period = ?2
         ORDER BY e.score {order}, e.updated_ms ASC, e.account_id ASC
         LIMIT ?3"
    ))?;
    let rows = stmt
        .query_map(params![board, key, limit.clamp(1, MAX_LIMIT)], |r| {
            Ok((
                r.get::<_, AccountId>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, f64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Leaderboard {
        board: board.to_string(),
        period: key,
        ascending: asc,
        rows: rows
            .into_iter()
            .enumerate()
            .map(|(i, (account_id, display_name, score))| LeaderboardRow {
                rank: i as u32 + 1,
                account_id,
                display_name,
                score,
            })
            .collect(),
    })
}

/// Delete the current period of a board (admin reset). Returns rows removed.
pub fn reset(conn: &Connection, board: &str, period: Period, now: UnixMs) -> ApiResult<usize> {
    validate_board(board)?;
    Ok(conn.execute(
        "DELETE FROM lb_entries WHERE board = ?1 AND period = ?2",
        params![board, period.key(now)],
    )?)
}

/// Placement of one racer for rating purposes. Lower `rank_key` = better.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Racer.
    pub account: AccountId,
    /// Rating before the race.
    pub rating: f64,
    /// Ordering key: clean finishers by position, then dirty finishers, then DNFs (all equal).
    pub rank_key: u64,
}

/// Ordering key for a result: clean finishes beat every dirty finish; DNFs tie last.
pub fn rank_key(position: u32, clean: bool) -> u64 {
    match (position, clean) {
        (0, _) => u64::MAX,
        (p, true) => p as u64,
        (p, false) => 1_000_000 + p as u64,
    }
}

/// Multi-player Elo: every pair of racers is a game (win / loss / draw by `rank_key`), and the
/// K-factor is shared over the `n - 1` opponents. Order of `field` does not matter.
pub fn rating_changes(field: &[Placement]) -> Vec<f64> {
    let n = field.len();
    if n < 2 {
        return vec![0.0; n];
    }
    let k = RATING_K / (n - 1) as f64;
    field
        .iter()
        .map(|me| {
            let sum: f64 = field
                .iter()
                .filter(|o| o.account != me.account)
                .map(|o| {
                    let score = match me.rank_key.cmp(&o.rank_key) {
                        std::cmp::Ordering::Less => 1.0,
                        std::cmp::Ordering::Equal => 0.5,
                        std::cmp::Ordering::Greater => 0.0,
                    };
                    let expected = 1.0 / (1.0 + 10f64.powf((o.rating - me.rating) / 400.0));
                    score - expected
                })
                .sum();
            k * sum
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(account: AccountId, rating: f64, pos: u32, clean: bool) -> Placement {
        Placement {
            account,
            rating,
            rank_key: rank_key(pos, clean),
        }
    }

    #[test]
    fn winner_gains_loser_loses_zero_sum() {
        let d = rating_changes(&[p(1, 1500.0, 1, true), p(2, 1500.0, 2, true)]);
        assert!((d[0] - 16.0).abs() < 1e-9);
        assert!((d[1] + 16.0).abs() < 1e-9);
        let field = [
            p(1, 1600.0, 2, true),
            p(2, 1400.0, 1, false),
            p(3, 1500.0, 0, true),
            p(4, 1550.0, 3, true),
        ];
        let d = rating_changes(&field);
        assert!(d.iter().sum::<f64>().abs() < 1e-9, "zero-sum");
        assert!(d[0] > 0.0, "clean P2 beats the dirty winner");
        assert!(d[2] < 0.0, "DNF loses");
    }

    #[test]
    fn dirty_win_ranks_behind_clean_finish() {
        assert!(rank_key(5, true) < rank_key(1, false));
        assert!(rank_key(1, false) < rank_key(0, true));
        assert_eq!(rank_key(0, true), rank_key(0, false));
    }

    #[test]
    fn boards_validate() {
        assert!(validate_board("bounty").is_ok());
        assert!(validate_board("time_attack:docks-loop_2").is_ok());
        assert!(validate_board("time_attack:").is_err());
        assert!(validate_board("time_attack:Bad Route").is_err());
        assert!(validate_board("nope").is_err());
    }
}
