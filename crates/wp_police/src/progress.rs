//! Despawning AI units that are getting nowhere (owner request 2026-10-23: "AI police that are
//! not making meaningful progress and not in view of any racers despawn").
//!
//! [`crate::stuck`] catches a car that wants to move and can't. This catches the rest: a unit
//! that moves but never gets any closer to where it is going (lost, circling a block, chasing
//! a racer that is pulling away, re-routing back and forth, pinned somewhere the stuck
//! detector doesn't see as stuck).
//!
//! Every AI ground unit has a goal while it has a job ([`goal_of`] in `dispatch`): the racer
//! it pursues or was called about, the crash it was sent to, its intercept point or its
//! patrol destination. Getting [`GAIN_M`] closer to the goal than it has been since the job
//! began (or since the last such gain) is **meaningful progress**; so is being within
//! [`ARRIVED_M`] of the goal (on scene, waiting at an ambush point). A unit with no goal
//! (idle) makes progress by driving [`MOVE_M`] from where it last did. A unit that has made
//! no progress for [`NO_PROGRESS_S`] gives up and despawns once out of every racer's sight
//! (and at least `response.despawn_min_m` away), exactly like a stuck unit: its call is
//! dispatched again and a patrol is refilled out of sight. While a racer can see it, it keeps
//! trying.
//!
//! Player units, parked tool units (roadblocks, spikes), helicopters and units already out of
//! service are never watched. Same behaviour in both damage modes.

use serde::{Deserialize, Serialize};

use crate::graph::dist;
use crate::time::SimTime;

/// No meaningful progress for this long gives up, s.
pub const NO_PROGRESS_S: f32 = 30.0;
/// Getting this much closer to the goal than the best so far is meaningful progress, m.
pub const GAIN_M: f32 = 25.0;
/// Within this of the goal counts as progress (arrived; waiting there is the job), m.
pub const ARRIVED_M: f32 = 40.0;
/// A unit without a goal makes progress by driving this far from where it last did, m.
pub const MOVE_M: f32 = 40.0;

/// Per-unit progress watch.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct ProgressWatch {
    /// The job being watched: when it began (`Unit::assigned`) and which leg of it (a patrol
    /// picks its next destination without a new assignment); a new job restarts the watch.
    pub(crate) job: (SimTime, u32),
    /// Whether the watch has been started for `job`.
    pub(crate) started: bool,
    /// Last meaningful progress.
    pub(crate) since: SimTime,
    /// Closest the unit has been to its goal since `since`, m (`f32::INFINITY`: no goal yet).
    pub(crate) best_m: f32,
    /// Where it was at `since` (goal-less progress).
    pub(crate) anchor: [f32; 2],
}

impl ProgressWatch {
    /// Note the unit at `pos` on job `job` (see [`ProgressWatch::job`]), `goal_m` from its goal
    /// (`None`: no goal). Returns whether it has gone [`NO_PROGRESS_S`] without meaningful progress.
    pub(crate) fn update(
        &mut self,
        now: SimTime,
        job: (SimTime, u32),
        pos: [f32; 2],
        goal_m: Option<f32>,
    ) -> bool {
        if !self.started || self.job != job {
            self.restart(now, job, pos, goal_m);
            return false;
        }
        let progressed = match goal_m {
            Some(d) => d <= ARRIVED_M || d <= self.best_m - GAIN_M,
            None => dist(self.anchor, pos) >= MOVE_M,
        };
        if progressed {
            self.restart(now, job, pos, goal_m);
            return false;
        }
        now.since(self.since) >= NO_PROGRESS_S
    }

    /// Forget the watch (the unit is not watched right now).
    pub(crate) fn clear(&mut self) {
        self.started = false;
    }

    fn restart(&mut self, now: SimTime, job: (SimTime, u32), pos: [f32; 2], goal_m: Option<f32>) {
        *self = Self {
            job,
            started: true,
            since: now,
            best_m: goal_m.unwrap_or(f32::INFINITY),
            anchor: pos,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: f32) -> SimTime {
        SimTime::ZERO.plus(s)
    }

    #[test]
    fn closing_in_is_progress_holding_distance_is_not() {
        let mut w = ProgressWatch::default();
        let job = (t(1.0), 0);
        assert!(!w.update(t(1.0), job, [0.0, 0.0], Some(500.0)));
        // Closing 30 m every 10 s: never gives up.
        for k in 1..20 {
            let d = 500.0 - 30.0 * k as f32;
            assert!(!w.update(t(1.0 + 10.0 * k as f32), job, [0.0, 0.0], Some(d)));
        }
        // Holding at 300 m (a racer pulling away at the same pace) gives up after the window.
        let mut w = ProgressWatch::default();
        assert!(!w.update(t(0.0), job, [0.0, 0.0], Some(300.0)));
        assert!(!w.update(t(NO_PROGRESS_S - 1.0), job, [50.0, 0.0], Some(290.0)));
        assert!(w.update(t(NO_PROGRESS_S), job, [90.0, 0.0], Some(285.0)));
    }

    #[test]
    fn arrived_and_new_jobs_reset() {
        let mut w = ProgressWatch::default();
        assert!(!w.update(t(0.0), (t(0.0), 0), [0.0, 0.0], Some(30.0)));
        // Waiting at the goal (ambush point, crash scene) is the job.
        assert!(!w.update(t(100.0), (t(0.0), 0), [0.0, 0.0], Some(30.0)));
        // A new job restarts the clock.
        assert!(!w.update(t(101.0), (t(101.0), 0), [0.0, 0.0], Some(800.0)));
        assert!(!w.update(
            t(101.0 + NO_PROGRESS_S - 0.5),
            (t(101.0), 0),
            [0.0, 0.0],
            Some(800.0)
        ));
        assert!(!w.update(t(200.0), (t(200.0), 0), [0.0, 0.0], Some(800.0)));
        // So does the next leg of the same job (a patrol's next destination).
        assert!(!w.update(t(220.0), (t(200.0), 0), [0.0, 0.0], Some(20.0)));
        assert!(!w.update(t(221.0), (t(200.0), 7), [0.0, 0.0], Some(600.0)));
        assert!(!w.update(
            t(221.0 + NO_PROGRESS_S - 0.5),
            (t(200.0), 7),
            [0.0, 0.0],
            Some(560.0)
        ));
    }

    #[test]
    fn goal_less_units_must_drive_somewhere() {
        let mut w = ProgressWatch::default();
        assert!(!w.update(t(0.0), (t(0.0), 0), [0.0, 0.0], None));
        // Shuffling back and forth within 30 m of one spot.
        let mut gave_up = false;
        for k in 1..=40 {
            let x = if k % 2 == 0 { 0.0 } else { 30.0 };
            gave_up = w.update(t(k as f32), (t(0.0), 0), [x, 0.0], None);
        }
        assert!(gave_up);
        // Driving along a road never gives up.
        let mut w = ProgressWatch::default();
        for k in 0..100 {
            let a = k as f32;
            assert!(!w.update(t(a), (t(0.0), 0), [0.0, 10.0 * a], None));
        }
    }
}
