//! Jam diagnostics (owner playtest row 85: "massive traffic jam" behind a red light during a
//! pursuit). Measures, it does not steer anything:
//!
//! - **Stop time outside red lights** per agent (`Agent::jam_s`): grows while the car
//!   stands still (< [`STOPPED_MPS`]) and is held, not reset, while its lane shows red or
//!   amber, so a car that sits through several cycles without getting through counts the green
//!   time it lost. Reset once it moves. [`TrafficStats::max_stop_s`] is the longest seen
//!   (wrecked cars are left out: a wreck is not a jam; the metric's timer is also held while
//!   the car yields to a siren, like at a red, and within
//!   [`SCENE_RADIUS_M`] of a standing scene — a tracked vehicle stopped for [`SCENE_STILL_S`],
//!   which in the game ends the run when it is a capture — while the behaviour timer runs on).
//! - **Queue length** ([`TrafficStats::max_queue`]), sampled every [`SAMPLE_STEPS`]: stopped cars
//!   back from the front of each lane (gaps < [`QUEUE_GAP_M`]), followed back through the
//!   junction into the lanes feeding it while the queue reaches the lane start.
//! - **Stopped in the junction box** ([`TrafficStats::box_stops`]): cars that stood inside a
//!   junction connector longer than [`BOX_STOP_S`] (a blocked box or a deadlocked turn).

use super::*;

/// A tracked vehicle stopped this long is a standing scene (capture, parked player), s.
pub const SCENE_STILL_S: f32 = 20.0;
/// The jam metric's stop timer is held this close to a standing scene, m (the cars boxed in
/// there wait for the scene to clear; in the game a capture ends the run).
pub const SCENE_RADIUS_M: f32 = 60.0;
/// A car slower than this counts as stopped, m/s.
pub const STOPPED_MPS: f32 = 0.5;
/// Queue sampling period, traffic steps (1 s).
pub const SAMPLE_STEPS: u64 = 60;
/// Largest gap between queued cars (bumper to bumper is ~2 m; 12 m allows a car-length hole), m.
pub const QUEUE_GAP_M: f32 = 12.0 + CAR_LENGTH_M;
/// A car standing inside a junction this long counts once in [`TrafficStats::box_stops`], s.
pub const BOX_STOP_S: f32 = 5.0;
/// A queue is followed back through at most this many junctions.
const MAX_CHAIN: usize = 6;

impl Traffic {
    /// Update an agent's stop-time counters (called once per traffic step per agent).
    pub(super) fn update_jam_time(&mut self, map: &CityMap, i: usize, dt: f32) {
        let a = &mut self.agents[i];
        if a.v >= STOPPED_MPS {
            a.jam_s = 0.0;
            a.box_s = 0.0;
            a.booked_s = 0.0;
            a.book_room_s = 0.0;
            a.metric_stop_s = 0.0;
            return;
        }
        let seg = map.seg(a.seg);
        if seg.is_connector() {
            a.box_s += dt;
            if a.box_s >= BOX_STOP_S && a.box_s - dt < BOX_STOP_S {
                self.stats.box_stops += 1;
            }
        }
        let red = seg
            .lane()
            .and_then(|l| map.lane_signal(l))
            .is_some_and(|(st, _)| matches!(st, SignalState::Red | SignalState::Yellow));
        if !red {
            a.jam_s += dt;
        }
        let q = geom::xy(self.poses[i].p);
        let at_scene = self
            .scenes
            .iter()
            .any(|p| geom::dist(*p, q) < SCENE_RADIUS_M);
        let a = &mut self.agents[i];
        // Held for a siren (pulled over, or kept from entering the junction while a unit comes
        // through) is like a red light: the law, not a jam.
        let siren = a.yielding || a.siren_hold_s > 0.0;
        if !red && !at_scene && !siren {
            a.metric_stop_s += dt;
        }
        // A wreck is not a jam (the cars behind it are, and they go around).
        if a.metric_stop_s > self.stats.max_stop_s && !a.disabled {
            self.stats.max_stop_s = a.metric_stop_s;
        }
    }

    /// Sample queue lengths (call after `rebuild_lists`).
    pub(super) fn sample_queues(&mut self, map: &CityMap) {
        if !self.step_count.is_multiple_of(SAMPLE_STEPS) {
            return;
        }
        let mut best = 0u32;
        for seg in 0..map.segs.len() {
            if map.segs[seg].is_connector() {
                continue;
            }
            let mut visited = Vec::new();
            let q = self.queue_back(map, seg as SegId, 0, &mut visited);
            best = best.max(q);
        }
        self.stats.max_queue = self.stats.max_queue.max(best);
    }

    /// Stopped cars queued back from the front of lane `seg` (and, while the queue reaches the
    /// lane start, from the lanes feeding it through the junction behind).
    fn queue_back(&self, map: &CityMap, seg: SegId, depth: usize, visited: &mut Vec<SegId>) -> u32 {
        if depth > MAX_CHAIN || visited.contains(&seg) {
            return 0;
        }
        visited.push(seg);
        let sg = map.seg(seg);
        let list = &self.lists[seg as usize];
        let mut n = 0u32;
        // Front of the queue: where the next car would stop (the stop line, or the car ahead
        // in the junction box).
        let mut front = sg.length + CAR_LENGTH_M;
        for e in list.iter().rev().filter(|e| !e.ghost) {
            if e.v >= STOPPED_MPS || front - e.s > QUEUE_GAP_M {
                return n;
            }
            n += 1;
            front = e.s;
        }
        // The queue reaches back to the start of the lane: continue into the feeding lanes.
        if front > QUEUE_GAP_M {
            return n;
        }
        let mut more = 0u32;
        for &c in &sg.prev {
            if !map.seg(c).is_connector() {
                continue;
            }
            for &p in &map.seg(c).prev {
                more = more.max(self.queue_back(map, p, depth + 1, visited));
            }
        }
        n + more
    }
}
