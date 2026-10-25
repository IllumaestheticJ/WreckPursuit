//! Heat, wanted levels, bounty, and the escape / capture / safehouse rules.
//!
//! Everything here is a pure function of its inputs and the tables in `heat.toml`, so the
//! rules can be unit-tested without a city. [`crate::PoliceSystem`] gathers the inputs
//! (line of sight, nearest unit, safehouse occupancy) and calls [`RacerRecord::update`].

use serde::{Deserialize, Serialize};
use wp_contracts::{DayPhase, EntityId, ViolationKind};

use crate::config::HeatFile;
use crate::graph::District;
use crate::time::SimTime;

/// Wanted level derived from heat (moved to `wp_contracts` in v0.2.0; re-exported here).
pub use wp_contracts::WantedLevel;

/// Where a racer stands with the police.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum PursuitStatus {
    /// No heat worth mentioning.
    #[default]
    Clear,
    /// Wanted (heat ≥ L1) but no active pursuit.
    Wanted,
    /// Actively pursued with line of sight.
    Pursuit,
    /// Pursuit active but contact lost; escape timer running.
    Searching,
}

/// Rule outcome from [`RacerRecord::update`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleOutcome {
    /// Escape timer ran out.
    Escaped,
    /// Quiet safehouse dwell completed.
    Safehouse,
    /// Stopped next to a unit.
    Captured,
}

/// Persistent bounty ledger (prompt 08 stores it across sessions).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct BountyRecord {
    /// ∫ heat/100 dt while wanted, s.
    pub heat_seconds: f32,
    /// Races won while wanted.
    pub races_won_wanted: u32,
    /// Police units disabled.
    pub cops_disabled: u32,
}

impl BountyRecord {
    /// Bounty value from the ledger.
    pub fn bounty(&self, cfg: &HeatFile) -> f32 {
        let b = &cfg.bounty;
        self.heat_seconds * b.per_heat_second
            + self.races_won_wanted as f32 * b.per_race_won_wanted
            + self.cops_disabled as f32 * b.per_cop_disabled
    }

    /// Reinforcement priority 0..=1 (1 at `max_priority_bounty`).
    pub fn priority(&self, cfg: &HeatFile) -> f32 {
        (self.bounty(cfg) / cfg.bounty.max_priority_bounty.max(1.0)).clamp(0.0, 1.0)
    }
}

/// Heat for one violation before the witness factor.
pub fn violation_heat(kind: &ViolationKind, cfg: &HeatFile) -> f32 {
    let w = &cfg.heat.violation;
    match kind {
        ViolationKind::Speeding { over_mps } => {
            w.speeding_base + w.speeding_per_mps * over_mps.max(0.0)
        }
        ViolationKind::RedLight => w.red_light,
        ViolationKind::WrongWay => w.wrong_way,
        ViolationKind::Collision => w.collision,
        ViolationKind::Reckless => w.reckless,
    }
}

/// Index of a violation kind for per-kind cooldowns.
pub fn violation_index(kind: &ViolationKind) -> usize {
    match kind {
        ViolationKind::Speeding { .. } => 0,
        ViolationKind::RedLight => 1,
        ViolationKind::WrongWay => 2,
        ViolationKind::Collision => 3,
        ViolationKind::Reckless => 4,
    }
}

/// Seconds without line of sight needed to escape at `level` during `phase`.
pub fn escape_time_s(level: WantedLevel, phase: DayPhase, cfg: &HeatFile) -> f32 {
    cfg.escape.base_s[level.table_index()] * cfg.escape.phase_scale.get(phase)
}

/// What the police system observed about a racer this think step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RuleInputs {
    /// Any unit, camera or helicopter has line of sight.
    pub seen: bool,
    /// Distance to the nearest ground unit, m (`f32::INFINITY` if none).
    pub nearest_unit_m: f32,
    /// Distance to the nearest ground unit chasing this racer (`Pursue` / `Intercept` on it),
    /// m. The pinned rule counts only those: one unit stopped for its own target is not
    /// pinning the queue behind that target (Hollins hairpin: a cop holding for a racer
    /// head-on had all three queued racers arrested).
    pub nearest_chaser_m: f32,
    /// Racer is inside a safehouse POI.
    pub in_safehouse: bool,
    /// Stopped behind another racer that is closer ahead than any unit (a race field queued
    /// on a one-lane road, row 168): neither the stop nor the pinned rule counts while it
    /// waits. A box-in or a unit right in front still captures.
    pub held_up_by_racer: bool,
    /// Every unit that counts for the stop rule (within `unit_radius_m`) and the pinned rule
    /// (a chaser within `pinned_unit_radius_m`) stands in the racer's lane in front of it, a
    /// pursuer, interceptor or patrol (row 168): both rules then take at least
    /// `blocked_ahead_stop_time_s`.
    pub blocked_ahead_only: bool,
    /// Racer speed, m/s.
    pub speed_mps: f32,
    /// District the racer is in.
    pub district: District,
    /// Day phase.
    pub phase: DayPhase,
    /// Simulation time.
    pub now: SimTime,
}

/// Per-racer police state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RacerRecord {
    /// Racer vehicle.
    pub entity: EntityId,
    /// Latest position, m.
    pub pos: [f32; 3],
    /// Latest velocity, m/s.
    pub vel: [f32; 3],
    /// Heat 0..=max.
    pub heat: f32,
    /// Wanted level from heat.
    pub level: WantedLevel,
    /// Pursuit status.
    pub status: PursuitStatus,
    /// Last time any unit/camera saw the racer (`None` = never).
    pub last_seen: Option<SimTime>,
    /// Last position the police know, m.
    pub last_known: Option<[f32; 2]>,
    /// Velocity at the last sighting, m/s (where a search heads; the racer's real velocity
    /// once out of sight is not police knowledge).
    #[serde(default)]
    pub last_known_vel: [f32; 2],
    /// Seconds since line of sight was lost (Searching).
    pub los_lost_s: f32,
    /// Seconds stopped near a unit.
    pub stopped_s: f32,
    /// Where the racer has stayed near since `pinned_s` started counting, m.
    #[serde(default)]
    pub pin_anchor: Option<[f32; 2]>,
    /// Seconds held within `capture.pinned_radius_m` of `pin_anchor` with a unit close.
    #[serde(default)]
    pub pinned_s: f32,
    /// Seconds of quiet safehouse dwell.
    pub safehouse_s: f32,
    /// Last time each violation kind counted.
    pub last_violation: [Option<SimTime>; 5],
    /// Bounty ledger.
    pub bounty: BountyRecord,
    /// When the current pursuit started.
    pub pursuit_started: Option<SimTime>,
}

impl RacerRecord {
    /// Fresh record (no heat).
    pub fn new(entity: EntityId) -> Self {
        Self {
            entity,
            pos: [0.0; 3],
            vel: [0.0; 3],
            heat: 0.0,
            level: WantedLevel::None,
            status: PursuitStatus::Clear,
            last_seen: None,
            last_known: None,
            last_known_vel: [0.0, 0.0],
            los_lost_s: 0.0,
            stopped_s: 0.0,
            pin_anchor: None,
            pinned_s: 0.0,
            safehouse_s: 0.0,
            last_violation: [None; 5],
            bounty: BountyRecord::default(),
            pursuit_started: None,
        }
    }

    /// Whether a pursuit is active (`Pursuit` or `Searching`).
    pub fn pursued(&self) -> bool {
        matches!(
            self.status,
            PursuitStatus::Pursuit | PursuitStatus::Searching
        )
    }

    /// Add a violation. `seen` = witnessed by a unit or camera. Returns heat added (0 if the
    /// same kind is still on cooldown).
    pub fn add_violation(
        &mut self,
        kind: &ViolationKind,
        seen: bool,
        now: SimTime,
        cfg: &HeatFile,
    ) -> f32 {
        let i = violation_index(kind);
        if self.last_violation[i].is_some_and(|t| now.since(t) < cfg.heat.repeat_cooldown_s) {
            return 0.0;
        }
        self.last_violation[i] = Some(now);
        let factor = if seen { 1.0 } else { cfg.heat.unseen_factor };
        let h = violation_heat(kind, cfg) * factor;
        self.add_heat(h, cfg);
        h
    }

    /// Add raw heat (clamped) and refresh the level.
    pub fn add_heat(&mut self, h: f32, cfg: &HeatFile) {
        self.heat = (self.heat + h).clamp(0.0, cfg.heat.max_heat);
        self.level = WantedLevel::from_heat(self.heat, &cfg.heat.level_thresholds);
    }

    /// Mark the start of a pursuit.
    pub fn start_pursuit(&mut self, now: SimTime) {
        self.status = PursuitStatus::Pursuit;
        self.los_lost_s = 0.0;
        self.stopped_s = 0.0;
        self.pin_anchor = None;
        self.pinned_s = 0.0;
        self.pursuit_started = Some(now);
    }

    /// End the pursuit (status falls back to Wanted/Clear by heat).
    pub fn end_pursuit(&mut self) {
        self.status = if self.level > WantedLevel::None {
            PursuitStatus::Wanted
        } else {
            PursuitStatus::Clear
        };
        self.los_lost_s = 0.0;
        self.stopped_s = 0.0;
        self.pin_anchor = None;
        self.pinned_s = 0.0;
        self.safehouse_s = 0.0;
        self.pursuit_started = None;
    }

    /// Advance the rules by `dt` seconds. Returns an outcome when a pursuit-ending rule fires
    /// (the caller ends the pursuit and adjusts heat).
    pub fn update(&mut self, inp: &RuleInputs, dt: f32, cfg: &HeatFile) -> Option<RuleOutcome> {
        if inp.seen {
            self.last_seen = Some(inp.now);
            self.last_known = Some([self.pos[0], self.pos[1]]);
            self.last_known_vel = [self.vel[0], self.vel[1]];
        }
        // Bounty integrates heat while wanted.
        if self.level > WantedLevel::None {
            self.bounty.heat_seconds += self.heat / 100.0 * dt;
        }
        // Heat: escalate while seen in pursuit, decay while unseen and not pursued.
        if self.pursued() && inp.seen {
            self.add_heat(cfg.heat.pursuit_heat_per_s * dt, cfg);
        } else if !self.pursued() {
            let since_seen = self
                .last_seen
                .map(|t| inp.now.since(t))
                .unwrap_or(f32::INFINITY);
            if since_seen > cfg.heat.decay_grace_s {
                let rate = cfg.heat.decay_per_s.get(inp.phase)
                    * cfg.heat.decay_district_scale.get(inp.district);
                self.add_heat(-rate * dt, cfg);
            }
        }

        let mut outcome = None;
        if self.pursued() {
            // Escape: no line of sight for escape_time.
            if inp.seen {
                self.status = PursuitStatus::Pursuit;
                self.los_lost_s = 0.0;
            } else {
                self.status = PursuitStatus::Searching;
                self.los_lost_s += dt;
                if self.los_lost_s >= escape_time_s(self.level, inp.phase, cfg) {
                    outcome = Some(RuleOutcome::Escaped);
                }
            }
            // Capture: stopped with a unit close by (not while queued behind another racer).
            if inp.speed_mps < cfg.capture.stop_speed_mps
                && inp.nearest_unit_m <= cfg.capture.unit_radius_m
                && !inp.held_up_by_racer
            {
                self.stopped_s += dt;
                let need = if inp.blocked_ahead_only {
                    cfg.capture
                        .stop_time_s
                        .max(cfg.capture.blocked_ahead_stop_time_s)
                } else {
                    cfg.capture.stop_time_s
                };
                if self.stopped_s >= need {
                    outcome = Some(RuleOutcome::Captured);
                }
            } else {
                self.stopped_s = 0.0;
            }
            // Pinned: boxed in and rocking back and forth, never getting away.
            let p = [self.pos[0], self.pos[1]];
            let close =
                inp.nearest_chaser_m <= cfg.capture.pinned_unit_radius_m && !inp.held_up_by_racer;
            match self.pin_anchor {
                Some(a)
                    if close
                        && (p[0] - a[0]) * (p[0] - a[0]) + (p[1] - a[1]) * (p[1] - a[1])
                            <= cfg.capture.pinned_radius_m * cfg.capture.pinned_radius_m =>
                {
                    self.pinned_s += dt;
                    let need = if inp.blocked_ahead_only {
                        cfg.capture
                            .pinned_time_s
                            .max(cfg.capture.blocked_ahead_stop_time_s)
                    } else {
                        cfg.capture.pinned_time_s
                    };
                    if self.pinned_s >= need {
                        outcome = Some(RuleOutcome::Captured);
                    }
                }
                _ => {
                    self.pin_anchor = Some(p);
                    self.pinned_s = 0.0;
                }
            }
        }
        // Safehouse: inside for dwell_s with no unit within clear_radius (any wanted state).
        if self.level > WantedLevel::None
            && inp.in_safehouse
            && inp.nearest_unit_m > cfg.safehouse.clear_radius_m
        {
            self.safehouse_s += dt;
            if self.safehouse_s >= cfg.safehouse.dwell_s && outcome.is_none() {
                outcome = Some(RuleOutcome::Safehouse);
            }
        } else {
            self.safehouse_s = 0.0;
        }
        if !self.pursued() {
            self.status = if self.level > WantedLevel::None {
                PursuitStatus::Wanted
            } else {
                PursuitStatus::Clear
            };
        }
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PoliceConfig;

    fn cfg() -> HeatFile {
        PoliceConfig::builtin().heat
    }

    fn inputs(seen: bool, nearest: f32, speed: f32, now: f32) -> RuleInputs {
        RuleInputs {
            seen,
            nearest_unit_m: nearest,
            nearest_chaser_m: nearest,
            in_safehouse: false,
            held_up_by_racer: false,
            blocked_ahead_only: false,
            speed_mps: speed,
            district: District::Core,
            phase: DayPhase::Day,
            now: SimTime::from_secs(now),
        }
    }

    #[test]
    fn levels_from_thresholds() {
        let c = cfg();
        let t = &c.heat.level_thresholds;
        assert_eq!(WantedLevel::from_heat(0.0, t), WantedLevel::None);
        assert_eq!(WantedLevel::from_heat(t[0], t), WantedLevel::L1);
        assert_eq!(WantedLevel::from_heat(t[2] + 0.1, t), WantedLevel::L3);
        assert_eq!(WantedLevel::from_heat(100.0, t), WantedLevel::L5);
    }

    #[test]
    fn unseen_violations_count_twenty_percent_and_respect_cooldown() {
        let c = cfg();
        let mut r = RacerRecord::new(EntityId(1));
        let seen = r.add_violation(&ViolationKind::RedLight, true, SimTime::ZERO, &c);
        let mut r2 = RacerRecord::new(EntityId(2));
        let unseen = r2.add_violation(&ViolationKind::RedLight, false, SimTime::ZERO, &c);
        assert!((unseen - seen * c.heat.unseen_factor).abs() < 1e-5);
        assert_eq!(
            r.add_violation(&ViolationKind::RedLight, true, SimTime::from_secs(0.5), &c),
            0.0
        );
        assert!(r.add_violation(&ViolationKind::RedLight, true, SimTime::from_secs(1.5), &c) > 0.0);
    }

    #[test]
    fn escape_fires_after_escape_time_without_los() {
        let c = cfg();
        let mut r = RacerRecord::new(EntityId(1));
        r.add_heat(35.0, &c); // L2
        r.start_pursuit(SimTime::ZERO);
        let need = escape_time_s(r.level, DayPhase::Day, &c);
        let dt = 0.1;
        let mut t = 0.0;
        let mut fired_at = None;
        while t < need + 5.0 {
            t += dt;
            if let Some(o) = r.update(&inputs(false, 500.0, 30.0, t), dt, &c) {
                assert_eq!(o, RuleOutcome::Escaped);
                fired_at = Some(t);
                break;
            }
        }
        let f = fired_at.expect("escape must fire");
        assert!((f - need).abs() < 0.2, "fired at {f}, need {need}");
    }

    #[test]
    fn regaining_los_resets_escape_timer() {
        let c = cfg();
        let mut r = RacerRecord::new(EntityId(1));
        r.add_heat(15.0, &c);
        r.start_pursuit(SimTime::ZERO);
        let need = escape_time_s(r.level, DayPhase::Day, &c);
        let dt = 0.1;
        let mut t = 0.0f32;
        // Lose contact for 80% of the escape time, regain, lose again: no escape yet.
        for _ in 0..((need * 0.8 / dt) as usize) {
            t += dt;
            assert!(r.update(&inputs(false, 300.0, 25.0, t), dt, &c).is_none());
        }
        assert_eq!(r.status, PursuitStatus::Searching);
        t += dt;
        assert!(r.update(&inputs(true, 50.0, 25.0, t), dt, &c).is_none());
        assert_eq!(r.status, PursuitStatus::Pursuit);
        for _ in 0..((need * 0.8 / dt) as usize) {
            t += dt;
            assert!(r.update(&inputs(false, 300.0, 25.0, t), dt, &c).is_none());
        }
    }

    /// Lead / opponent-pace (Hollins hairpin): a pursued racer stuck in a jam, the units held up
    /// behind other cars 20 m off (outside `unit_radius_m`), is captured after
    /// `pinned_time_s` instead of the whole road waiting for minutes.
    #[test]
    fn racer_stuck_in_a_jam_with_police_20_m_off_is_captured() {
        let c = cfg();
        assert!(c.capture.unit_radius_m < 20.0 && 20.0 < c.capture.pinned_unit_radius_m);
        let mut r = RacerRecord::new(EntityId(1));
        r.add_heat(15.0, &c);
        r.start_pursuit(SimTime::ZERO);
        let dt = 0.1;
        let mut t = 0.0;
        loop {
            t += dt;
            if let Some(o) = r.update(&inputs(true, 20.0, 0.3, t), dt, &c) {
                assert_eq!(o, RuleOutcome::Captured);
                break;
            }
            assert!(t < c.capture.pinned_time_s + 0.5, "no capture by {t} s");
        }
        assert!(
            (t - c.capture.pinned_time_s).abs() < 0.3,
            "captured at {t} s"
        );
    }

    #[test]
    fn racer_queued_behind_another_racer_is_not_captured_until_the_queue_clears() {
        // Row 168: a race field stopped behind its leader on a one-lane road, a unit 8 m off
        // (stop rule) and chasing it (pinned rule): waiting is not being caught.
        let c = cfg();
        let dt = 0.1;
        let mut r = RacerRecord::new(EntityId(1));
        r.add_heat(15.0, &c);
        r.start_pursuit(SimTime::ZERO);
        let queued = |now: f32| RuleInputs {
            held_up_by_racer: true,
            ..inputs(true, 8.0, 0.0, now)
        };
        for k in 0..300 {
            assert_eq!(
                r.update(&queued(k as f32 * dt), dt, &c),
                None,
                "captured at {k}"
            );
        }
        // The racer ahead drives off and this one stays put next to the unit: caught.
        let mut t = 0.0;
        loop {
            t += dt;
            if let Some(o) = r.update(&inputs(true, 8.0, 0.0, 30.0 + t), dt, &c) {
                assert_eq!(o, RuleOutcome::Captured);
                break;
            }
            assert!(t < c.capture.stop_time_s + 0.5, "no capture by {t} s");
        }
    }

    #[test]
    fn capture_needs_stop_time_and_nearby_unit() {
        let c = cfg();
        let mut r = RacerRecord::new(EntityId(1));
        r.add_heat(15.0, &c);
        r.start_pursuit(SimTime::ZERO);
        let dt = 0.1;
        // Stopped but the unit is too far: no capture.
        for i in 0..100 {
            assert!(r
                .update(
                    &inputs(
                        true,
                        c.capture.pinned_unit_radius_m + 5.0,
                        0.5,
                        i as f32 * 0.1
                    ),
                    dt,
                    &c
                )
                .is_none());
        }
        // Unit close: capture after stop_time_s.
        let mut t = 0.0;
        loop {
            t += dt;
            if let Some(o) = r.update(&inputs(true, 8.0, 0.5, 10.0 + t), dt, &c) {
                assert_eq!(o, RuleOutcome::Captured);
                break;
            }
            assert!(t < c.capture.stop_time_s + 0.5);
        }
        assert!((t - c.capture.stop_time_s).abs() < 0.2);
        // Rolling at 3 m/s never captures.
        let mut r = RacerRecord::new(EntityId(2));
        r.add_heat(15.0, &c);
        r.start_pursuit(SimTime::ZERO);
        for i in 0..200 {
            r.pos = [i as f32 * 0.3, 0.0, 0.0];
            assert!(r
                .update(&inputs(true, 5.0, 3.0, i as f32 * 0.1), dt, &c)
                .is_none());
        }
    }

    #[test]
    fn safehouse_requires_quiet_dwell() {
        let c = cfg();
        let mut r = RacerRecord::new(EntityId(1));
        r.add_heat(40.0, &c);
        r.start_pursuit(SimTime::ZERO);
        let dt = 0.1;
        let mut inp = inputs(false, 100.0, 0.0, 0.0);
        inp.in_safehouse = true;
        // A unit at 100 m (< 150 m) blocks the safehouse; nearest > capture radius so no capture.
        for i in 0..150 {
            inp.now = SimTime::from_secs(i as f32 * 0.1);
            let o = r.update(&inp, dt, &c);
            assert_ne!(o, Some(RuleOutcome::Safehouse));
            if o == Some(RuleOutcome::Escaped) {
                r.start_pursuit(inp.now);
            }
        }
        inp.nearest_unit_m = 400.0;
        let mut t = 0.0;
        loop {
            t += dt;
            inp.now = SimTime::from_secs(20.0 + t);
            let o = r.update(&inp, dt, &c);
            if o == Some(RuleOutcome::Safehouse) {
                break;
            }
            if o == Some(RuleOutcome::Escaped) {
                r.start_pursuit(inp.now);
            }
            assert!(t < c.safehouse.dwell_s + 0.5);
        }
        assert!((t - c.safehouse.dwell_s).abs() < 0.2);
    }

    #[test]
    fn decay_is_faster_at_night_and_in_suburb() {
        let c = cfg();
        let run = |phase, district| {
            let mut r = RacerRecord::new(EntityId(1));
            r.add_heat(50.0, &c);
            let mut inp = inputs(false, 1000.0, 10.0, 0.0);
            inp.phase = phase;
            inp.district = district;
            for i in 0..300 {
                inp.now = SimTime::from_secs(10.0 + i as f32 * 0.1);
                r.update(&inp, 0.1, &c);
            }
            r.heat
        };
        let day_core = run(DayPhase::Day, District::Core);
        assert!(run(DayPhase::Night, District::Core) < day_core);
        assert!(run(DayPhase::Day, District::Suburb) < day_core);
        assert!(day_core < 50.0);
    }

    #[test]
    fn bounty_grows_with_history() {
        let c = cfg();
        let mut b = BountyRecord::default();
        let b0 = b.bounty(&c);
        b.races_won_wanted += 1;
        b.cops_disabled += 2;
        b.heat_seconds += 30.0;
        assert!(b.bounty(&c) > b0);
        assert!(b.priority(&c) > 0.0 && b.priority(&c) <= 1.0);
    }

    /// Owner playtest row 63: a racer boxed in and rocking back and forth (never stopped
    /// for 4 s straight) with a unit close is captured once it has stayed within
    /// `pinned_radius_m` for `pinned_time_s`; one that drives away is not.
    #[test]
    fn pinned_racer_is_captured() {
        let c = cfg();
        let mut r = RacerRecord::new(EntityId(1));
        r.add_heat(40.0, &c);
        r.start_pursuit(SimTime::ZERO);
        let dt = 0.1;
        let mut out = None;
        for k in 0..((c.capture.pinned_time_s + 1.0) / dt) as usize {
            // 3 m/s back and forth over 2 m.
            let x = if (k / 7) % 2 == 0 {
                (k % 7) as f32 * 0.3
            } else {
                2.0 - (k % 7) as f32 * 0.3
            };
            r.pos = [x, 0.0, 0.0];
            out = out.or(r.update(&inputs(true, 5.0, 3.0, k as f32 * dt), dt, &c));
        }
        assert_eq!(out, Some(RuleOutcome::Captured));
        let mut r = RacerRecord::new(EntityId(1));
        r.add_heat(40.0, &c);
        r.start_pursuit(SimTime::ZERO);
        let mut out = None;
        for k in 0..((c.capture.pinned_time_s + 1.0) / dt) as usize {
            r.pos = [k as f32 * 0.3, 0.0, 0.0];
            out = out.or(r.update(&inputs(true, 5.0, 3.0, k as f32 * dt), dt, &c));
        }
        assert_eq!(out, None);
    }
}
