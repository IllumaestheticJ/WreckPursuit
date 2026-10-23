//! Instant crash replay (lead, owner request row 70): a rolling in-memory buffer of the live
//! session, big-crash detection for the player's car, and a playback session that shows the
//! crash again while the live session waits, untouched.
//!
//! **Buffer.** The session log ([`crate::session::Session`]) already holds every input and op
//! since tick 0. On top of that a live session keeps a snapshot (a clone of the [`Sim`], the
//! race judge and the per-car extras) every [`SNAPSHOT_EVERY_TICKS`], covering at least the
//! last [`BUFFER_TICKS`]. A crash replay starts from the newest snapshot at or before
//! "impact − 3 s" and re-simulates the logged inputs from there, so it shows exactly what
//! happened (its tick hashes are compared with the live ones).
//!
//! **Detection.** Every live tick the player's contacts are summed over a short window: a big
//! crash needs a closing speed of at least [`CrashTuning::min_closing_mps`] *and* either a
//! velocity change (normal impulse / mass) of [`CrashTuning::min_delta_v_mps`] or a health
//! drop of [`CrashTuning::min_health_drop`]. Scrapes (low closing speed) and hard landings
//! (vertical contacts with the ground, unless they damage the car) do not count. Off during
//! a race countdown, after the player's race is over and once the run holds the car.
//!
//! **Replay.** [`crate::session::Session::crash_replay`] builds a playback session for
//! `impact − 3 s .. impact + 2 s`. Ticks after the live tick at the time of the request are
//! not in the log yet: they continue with the last logged inputs (an extrapolation; live play
//! resumes from the real state). The browser swaps it in (`crash_replay_start`) and back
//! (`crash_replay_stop`); the live session is parked meanwhile, so its tick and state hash
//! are unchanged.

use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};
use wp_contracts::{DamageMode, EntityId, SimEvent, VehicleInput, VehicleState, WORLD_ENTITY};
use wp_sim::Sim;

use crate::race::RaceRun;
use crate::session::{world_to_local, EntityExtra};

/// Ticks between snapshots (2 s): a replay fast-forwards at most this far before it shows.
/// A snapshot of a night run costs ~6 ms native (4 ms of it Rapier's own sets), so not every
/// second.
pub const SNAPSHOT_EVERY_TICKS: u64 = 2 * 240;

/// The buffer covers at least the last 10 s of the live session.
pub const BUFFER_TICKS: u64 = 10 * 240;

/// A replay starts this long before the impact (3 s).
pub const PRE_IMPACT_TICKS: u64 = 3 * 240;

/// A replay ends this long after the impact (2 s).
pub const POST_IMPACT_TICKS: u64 = 2 * 240;

/// Big-crash thresholds (tunable at run time, `crash_tuning_set`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CrashTuning {
    /// Peak closing speed of a player contact in the window, m/s.
    pub min_closing_mps: f32,
    /// Summed normal impulse in the window / player car mass, m/s.
    pub min_delta_v_mps: f32,
    /// Health lost (0..1) on contact ticks in the window.
    pub min_health_drop: f32,
    /// Window the contacts are summed over, ticks.
    pub window_ticks: u64,
    /// No new crash this many ticks after the last one.
    pub cooldown_ticks: u64,
}

impl Default for CrashTuning {
    fn default() -> Self {
        Self {
            min_closing_mps: 7.0,
            min_delta_v_mps: 4.0,
            min_health_drop: 0.05,
            window_ticks: 48,
            cooldown_ticks: 6 * 240,
        }
    }
}

/// Which part of the player's car took the hit (from the contact point in chassis space).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImpactSide {
    /// Nose (+Y).
    Front,
    /// Tail (−Y).
    Rear,
    /// Left flank (−X).
    Left,
    /// Right flank (+X).
    Right,
    /// Roof / underside (a rollover).
    Top,
}

/// A detected big crash.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrashMark {
    /// Increments with every crash of the session.
    pub seq: u32,
    /// Tick of the strongest contact (the impact moment).
    pub tick: u64,
    /// The player's car.
    pub player: u32,
    /// Other entity of the strongest contact (`None` = static world).
    pub other: Option<u32>,
    /// Peak closing speed, m/s.
    pub closing_mps: f32,
    /// Summed normal impulse / mass, m/s.
    pub delta_v_mps: f32,
    /// Health lost on the window's contact ticks.
    pub health_drop: f32,
    /// World contact point of the strongest contact, m.
    pub point: [f32; 3],
    /// Unit vector from the car's centre to that point, chassis space.
    pub dir_local: [f32; 3],
    /// Hit side (picks the replay's camera shots).
    pub side: ImpactSide,
}

/// What a crash replay covers (JSON for the browser).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CrashReplayInfo {
    /// The crash.
    pub crash: CrashMark,
    /// First tick shown.
    pub start_tick: u64,
    /// Last tick shown.
    pub end_tick: u64,
    /// Live tick when the replay was built (ticks after it are extrapolated).
    pub live_tick: u64,
    /// Live state hash when the replay was built (hex); the same after it.
    pub live_hash: String,
}

/// One buffered snapshot (taken at the end of a live tick).
#[derive(Clone)]
pub(crate) struct Snapshot {
    pub(crate) tick: u64,
    pub(crate) sim: Sim,
    pub(crate) race: Option<RaceRun>,
    pub(crate) last_input: Vec<VehicleInput>,
    pub(crate) extras: BTreeMap<EntityId, EntityExtra>,
    pub(crate) mode: DamageMode,
}

/// One stepped live tick as the detector sees it.
pub(crate) struct PlayerTick<'a> {
    /// Tick just stepped to.
    pub(crate) now: u64,
    /// The player's car.
    pub(crate) player: EntityId,
    /// Its mass, kg.
    pub(crate) mass_kg: f32,
    /// Its state before the step.
    pub(crate) before: Option<&'a VehicleState>,
    /// Its state after the step.
    pub(crate) after: Option<&'a VehicleState>,
    /// The tick's sim events.
    pub(crate) events: &'a [SimEvent],
    /// The player may crash now (no race countdown, not finished, not held by the run).
    pub(crate) armed: bool,
}

/// One tick with player contacts.
#[derive(Debug, Clone, Copy)]
struct Sample {
    tick: u64,
    impulse_ns: f32,
    closing_mps: f32,
    health_drop: f32,
    /// Strongest contact this tick: impulse, other entity, world point.
    best: (f32, EntityId, [f32; 3]),
}

/// Snapshot ring + crash detector of a live session.
#[derive(Clone, Default)]
pub struct CrashRecorder {
    tuning: CrashTuning,
    /// Off: no snapshots, no detection (benches, replays).
    disabled: bool,
    snapshots: VecDeque<Snapshot>,
    window: VecDeque<Sample>,
    last_crash_tick: Option<u64>,
    seq: u32,
    pending: Option<CrashMark>,
    last: Option<CrashMark>,
}

impl CrashRecorder {
    /// A recorder that never snapshots or detects (playback sessions).
    pub fn off() -> Self {
        Self {
            disabled: true,
            ..Default::default()
        }
    }

    /// Thresholds in effect.
    pub fn tuning(&self) -> CrashTuning {
        self.tuning
    }

    /// Replace the thresholds.
    pub fn set_tuning(&mut self, t: CrashTuning) {
        self.tuning = t;
    }

    /// Turn snapshots + detection on or off (off drops the buffer).
    pub fn set_enabled(&mut self, on: bool) {
        self.disabled = !on;
        if !on {
            self.snapshots.clear();
            self.window.clear();
            self.pending = None;
        }
    }

    /// Whether snapshots + detection run.
    pub fn enabled(&self) -> bool {
        !self.disabled
    }

    /// Ticks of the buffered snapshots, oldest first.
    pub fn snapshot_ticks(&self) -> Vec<u64> {
        self.snapshots.iter().map(|s| s.tick).collect()
    }

    /// Take the crash detected since the last call, if any.
    pub fn take(&mut self) -> Option<CrashMark> {
        self.pending.take()
    }

    /// The latest crash of the session (kept after `take`).
    pub fn last(&self) -> Option<&CrashMark> {
        self.last.as_ref()
    }

    /// Drop every buffered snapshot.
    pub(crate) fn clear_snapshots(&mut self) {
        self.snapshots.clear();
    }

    /// Whether a snapshot is due after tick `now`.
    pub(crate) fn snapshot_due(&self, now: u64) -> bool {
        !self.disabled && now.is_multiple_of(SNAPSHOT_EVERY_TICKS)
    }

    /// Store a snapshot and drop the ones no longer needed for the last [`BUFFER_TICKS`].
    pub(crate) fn push_snapshot(&mut self, snap: Snapshot) {
        let now = snap.tick;
        self.snapshots.push_back(snap);
        while self.snapshots.len() >= 2 && self.snapshots[1].tick + BUFFER_TICKS <= now {
            self.snapshots.pop_front();
        }
    }

    /// Newest snapshot at or before `tick` (the oldest one if they are all later).
    pub(crate) fn snapshot_for(&self, tick: u64) -> Option<&Snapshot> {
        self.snapshots
            .iter()
            .rev()
            .find(|s| s.tick <= tick)
            .or_else(|| self.snapshots.front())
    }

    /// Feed one stepped live tick of the player's car.
    pub(crate) fn observe(&mut self, o: &PlayerTick) {
        let (now, player, mass_kg, events, armed) = (o.now, o.player, o.mass_kg, o.events, o.armed);
        let (before, after) = (o.before, o.after);
        if self.disabled {
            return;
        }
        let t = self.tuning;
        while self
            .window
            .front()
            .is_some_and(|s| s.tick + t.window_ticks <= now)
        {
            self.window.pop_front();
        }
        if !armed {
            self.window.clear();
            return;
        }
        let mut sample = Sample {
            tick: now,
            impulse_ns: 0.0,
            closing_mps: 0.0,
            health_drop: 0.0,
            best: (0.0, WORLD_ENTITY, [0.0; 3]),
        };
        let mut any = false;
        for ev in events {
            let SimEvent::Contact {
                a,
                b,
                point,
                normal,
                impulse_ns,
                rel_speed,
            } = ev
            else {
                continue;
            };
            let other = if *a == player {
                *b
            } else if *b == player {
                *a
            } else {
                continue;
            };
            any = true;
            // Landings: a vertical push from the static world only counts through damage.
            let landing = other == WORLD_ENTITY && normal[2].abs() > 0.8;
            if !landing {
                sample.impulse_ns += impulse_ns;
                sample.closing_mps = sample.closing_mps.max(rel_speed.abs());
            }
            if *impulse_ns > sample.best.0 {
                sample.best = (*impulse_ns, other, *point);
            }
        }
        if !any {
            return;
        }
        if let (Some(b), Some(a)) = (before, after) {
            sample.health_drop = (b.health - a.health).max(0.0);
        }
        self.window.push_back(sample);
        if self
            .last_crash_tick
            .is_some_and(|c| now < c + t.cooldown_ticks)
        {
            return;
        }
        let impulse: f32 = self.window.iter().map(|s| s.impulse_ns).sum();
        let closing = self
            .window
            .iter()
            .map(|s| s.closing_mps)
            .fold(0.0, f32::max);
        let drop: f32 = self.window.iter().map(|s| s.health_drop).sum();
        let dv = impulse / mass_kg.max(1.0);
        if closing < t.min_closing_mps || (dv < t.min_delta_v_mps && drop < t.min_health_drop) {
            return;
        }
        let peak = self
            .window
            .iter()
            .copied()
            .max_by(|x, y| x.best.0.total_cmp(&y.best.0))
            .unwrap_or(sample);
        let (pos, rot) = after.map_or(([0.0; 3], [0.0, 0.0, 0.0, 1.0]), |s| (s.pos, s.rot));
        let p = peak.best.2;
        let mut d = world_to_local(rot, [p[0] - pos[0], p[1] - pos[1], p[2] - pos[2]]);
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len > 1e-4 {
            d = [d[0] / len, d[1] / len, d[2] / len];
        }
        self.seq += 1;
        let mark = CrashMark {
            seq: self.seq,
            tick: peak.tick,
            player: player.0,
            other: (peak.best.1 != WORLD_ENTITY).then_some(peak.best.1 .0),
            closing_mps: closing,
            delta_v_mps: dv,
            health_drop: drop,
            point: p,
            dir_local: d,
            side: side_of(d),
        };
        self.last_crash_tick = Some(now);
        self.window.clear();
        self.last = Some(mark.clone());
        self.pending = Some(mark);
    }
}

/// Hit side from a chassis-space direction (centre → contact point). The car is longer than
/// wide, so the flanks get the wider share of the angle.
fn side_of(d: [f32; 3]) -> ImpactSide {
    if d[2].abs() > 0.75 {
        ImpactSide::Top
    } else if d[1] > 0.6 {
        ImpactSide::Front
    } else if d[1] < -0.6 {
        ImpactSide::Rear
    } else if d[0] < 0.0 {
        ImpactSide::Left
    } else {
        ImpactSide::Right
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contact(a: u32, b: u32, impulse: f32, speed: f32, nz: f32) -> SimEvent {
        SimEvent::Contact {
            a: EntityId(a),
            b: EntityId(b),
            point: [0.0, 2.0, 0.5],
            normal: [0.0, (1.0 - nz * nz).sqrt(), nz],
            impulse_ns: impulse,
            rel_speed: speed,
        }
    }

    fn state(health: f32) -> VehicleState {
        VehicleState {
            health,
            ..Default::default()
        }
    }

    /// Feed tick `now`: one contact, health `h0` → `h1`.
    fn feed(r: &mut CrashRecorder, now: u64, h: (f32, f32), ev: SimEvent, armed: bool) {
        let (b, a) = (state(h.0), state(h.1));
        r.observe(&PlayerTick {
            now,
            player: EntityId(1),
            mass_kg: 1300.0,
            before: Some(&b),
            after: Some(&a),
            events: &[ev],
            armed,
        });
    }

    #[test]
    fn hard_hit_is_a_crash_scrape_and_landing_are_not() {
        // Scrape: lots of impulse over time, low closing speed.
        let mut r = CrashRecorder::default();
        for t in 1..=40 {
            feed(&mut r, t, (1.0, 1.0), contact(1, 7, 400.0, 1.5, 0.0), true);
        }
        assert!(r.take().is_none(), "a scrape is not a crash");
        // Landing from a jump: fast vertical ground contact, no damage.
        let mut r = CrashRecorder::default();
        feed(
            &mut r,
            5,
            (1.0, 1.0),
            contact(1, u32::MAX, 9000.0, 9.0, 1.0),
            true,
        );
        assert!(r.take().is_none(), "a clean landing is not a crash");
        // Head-on into a car at 15 m/s.
        let mut r = CrashRecorder::default();
        feed(
            &mut r,
            9,
            (1.0, 0.9),
            contact(1, 7, 9000.0, 15.0, 0.0),
            true,
        );
        let m = r.take().expect("crash");
        assert_eq!((m.tick, m.other, m.side), (9, Some(7), ImpactSide::Front));
        // Cooldown: the next hit straight after is the same crash.
        feed(
            &mut r,
            20,
            (0.9, 0.8),
            contact(7, 1, 9000.0, 12.0, 0.0),
            true,
        );
        assert!(r.take().is_none());
        // Not armed (countdown / finished): nothing.
        let mut r = CrashRecorder::default();
        feed(
            &mut r,
            9,
            (1.0, 0.9),
            contact(1, 7, 9000.0, 15.0, 0.0),
            false,
        );
        assert!(r.take().is_none());
    }

    #[test]
    fn side_from_direction() {
        assert_eq!(side_of([0.0, 1.0, 0.0]), ImpactSide::Front);
        assert_eq!(side_of([0.0, -1.0, 0.0]), ImpactSide::Rear);
        assert_eq!(side_of([-0.9, 0.3, 0.0]), ImpactSide::Left);
        assert_eq!(side_of([0.9, -0.3, 0.0]), ImpactSide::Right);
        assert_eq!(side_of([0.0, 0.0, 1.0]), ImpactSide::Top);
    }
}
