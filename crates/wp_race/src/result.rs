//! Race results, DNF/DQ reasons and the total ordering used to rank them.
//!
//! Times are kept in integer **sub-ticks** (`1/1000` of a physics tick) so finishing order is
//! exact and identical on every target; seconds are derived for display only.

use serde::{Deserialize, Serialize};
use wp_contracts::{EntityId, PHYSICS_DT};

/// Sub-ticks per physics tick.
pub const SUBTICKS_PER_TICK: u64 = 1000;

/// Convert sub-ticks to seconds (display / payload only; never compare these).
pub fn subticks_to_s(subticks: u64) -> f32 {
    (subticks / SUBTICKS_PER_TICK) as f32 * PHYSICS_DT
        + (subticks % SUBTICKS_PER_TICK) as f32 * (PHYSICS_DT / SUBTICKS_PER_TICK as f32)
}

<<<<<<< HEAD
/// DNF / DQ reasons and racer status (contract types since v0.3.0).
pub use wp_contracts::{DnfReason, DqReason, RacerStatus};
=======
/// Why a racer did not finish (they still rank above every DQ).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DnfReason {
    /// The car became a total loss.
    TotalLoss,
    /// Arrested by police (reported by `wp_police` through the server).
    Captured,
    /// The race time limit ran out.
    TimeLimit,
    /// Did not finish within the window after the winner.
    FinishWindow,
    /// Quit / left the race voluntarily.
    Retired,
    /// Delivery cargo destroyed.
    CargoDestroyed,
    /// Lost an outrun (the opponent built the winning lead or led at the time limit).
    Outrun,
    /// Disconnected from the server.
    Disconnected,
    /// Abandoned the race to run from the police (AI entrants; see `entrants`).
    Fled,
}

/// Why a racer was disqualified (ranked below every finisher and every DNF).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum DqReason {
    /// Outside the route corridor longer than the grace time (shortcut).
    LeftCorridor,
    /// Moved before green (drag).
    JumpStart,
    /// Moved faster than physically plausible between two samples.
    ImplausibleMovement,
    /// Disqualified by the server / a marshal / an admin.
    Marshal,
}

/// A racer's state in the race.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RacerStatus {
    /// Entered; race not over for them yet.
    Running,
    /// Crossed the finish (or won the outrun).
    Finished,
    /// Did not finish.
    Dnf(DnfReason),
    /// Disqualified.
    Dq(DqReason),
}

impl RacerStatus {
    /// Ranking class: finishers 0, still running / DNF 1, DQ 2.
    pub fn class(&self) -> u8 {
        match self {
            RacerStatus::Finished => 0,
            RacerStatus::Running | RacerStatus::Dnf(_) => 1,
            RacerStatus::Dq(_) => 2,
        }
    }
}
>>>>>>> origin/ws/race

/// Drag-specific results.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct DragStats {
    /// Reaction time from green to launch, sub-ticks (`None` = never launched).
    pub reaction_subticks: Option<u64>,
    /// Upshifts made.
    pub shifts: u32,
    /// Upshifts inside the ideal window.
    pub good_shifts: u32,
    /// Speed when crossing the finish, m/s.
    pub trap_speed_mps: f32,
}

impl DragStats {
    /// Reaction time, s.
    pub fn reaction_s(&self) -> Option<f32> {
        self.reaction_subticks.map(subticks_to_s)
    }

    /// Fraction of upshifts inside the window (1.0 with no shifts).
    pub fn shift_score(&self) -> f32 {
        if self.shifts == 0 {
            1.0
        } else {
            self.good_shifts as f32 / self.shifts as f32
        }
    }
}

/// One racer's (provisional or final) result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RacerResult {
    /// The racer's vehicle entity.
    pub racer: EntityId,
    /// Start-grid slot (0 = pole). Final tie-break.
    pub grid_slot: u8,
    /// Final position, 1-based, unique.
    pub position: u8,
    /// Status.
    pub status: RacerStatus,
    /// Elapsed time from the racer's start to the finish, sub-ticks (finishers only).
    pub elapsed_subticks: Option<u64>,
    /// Best completed lap, sub-ticks (circuits).
    pub best_lap_subticks: Option<u64>,
    /// Gates passed in total (all laps).
    pub gates_passed: u32,
    /// Laps completed (circuits).
    pub laps_completed: u32,
    /// Distance along the course at the last sample (plus completed laps), m.
    pub progress_m: f32,
    /// Physics tick at which the status became final (0 while running).
    pub status_tick: u64,
    /// No contact above the clean threshold, no civilian hits, no collision violations,
    /// and the racer finished.
    pub clean: bool,
    /// Contacts above the clean threshold (racers, walls, police).
    pub contacts: u32,
    /// Contacts with civilians / traffic above the civilian threshold.
    pub civilian_hits: u32,
    /// Largest contact impulse seen, N·s.
    pub max_contact_ns: f32,
    /// Traffic violations by kind during the race: `[speeding, red light, wrong way,
    /// collision, reckless]`.
    pub violations: [u32; 5],
    /// Cargo health 0..=1 (1.0 when the race has no cargo).
    pub cargo_health: f32,
    /// Money owed for cargo damage (delivery only).
    pub cargo_damage_cost: i64,
    /// Drag stats (drag only).
    pub drag: Option<DragStats>,
}

impl From<&RacerResult> for wp_contracts::RaceResultRow {
    fn from(r: &RacerResult) -> Self {
        wp_contracts::RaceResultRow {
            racer: r.racer,
            position: r.position,
            status: r.status,
            elapsed_subticks: r.elapsed_subticks,
            best_lap_subticks: r.best_lap_subticks,
            clean: r.clean,
            contacts: r.contacts,
            civilian_hits: r.civilian_hits,
            cargo_damage_cost: r.cargo_damage_cost,
        }
    }
}

impl RacerResult {
    /// Elapsed time, s (display only).
    pub fn elapsed_s(&self) -> Option<f32> {
        self.elapsed_subticks.map(subticks_to_s)
    }

    /// Total-order ranking key: lower sorts first. Every component is an integer, so equal keys
    /// can only occur for the same racer (the last component is the entity id).
    ///
    /// Order: finishers by elapsed time; then running/DNF racers by gates passed, distance
    /// (cm) and how long they lasted; then DQs by the same; then grid slot; then entity id.
    pub fn rank_key(&self) -> (u8, u64, i64, i64, i64, u8, u32) {
        let class = self.status.class();
        if class == 0 {
            let time = self.elapsed_subticks.unwrap_or(u64::MAX);
            return (class, time, 0, 0, 0, self.grid_slot, self.racer.0);
        }
        let lasted = match self.status {
            RacerStatus::Running => i64::MAX,
            _ => self.status_tick as i64,
        };
        (
            class,
            0,
            -(self.gates_passed as i64),
            -((self.progress_m.max(0.0) * 100.0) as i64),
            -lasted,
            self.grid_slot,
            self.racer.0,
        )
    }
}

/// Sort results into final order and assign positions `1..=n`.
pub fn rank(results: &mut [RacerResult]) {
    results.sort_by_key(|r| r.rank_key());
    for (i, r) in results.iter_mut().enumerate() {
        r.position = (i + 1) as u8;
    }
}
