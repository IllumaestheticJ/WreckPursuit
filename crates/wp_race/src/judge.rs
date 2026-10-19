//! Server-side race judge: grid, countdown, checkpoint validation, laps, DNF/DQ, clean-race
//! detection, outrun gaps, drag timing and final ranking.
//!
//! The judge is pure and deterministic: it only sees what the server feeds it — racer samples
//! (from `wp_sim` vehicle states), the tick's `SimEvent`s and explicit calls such as
//! [`RaceJudge::dnf`] for police captures. Feed it every physics tick or every net tick; gate
//! tolerance scales with speed so a coarser rate still judges correctly.
//!
//! ```text
//! Lobby ──start(tick)──► Countdown ──tick ≥ green──► Running ──nobody running──► Finished
//! ```

use serde::{Deserialize, Serialize};
use wp_contracts::{EntityId, SimEvent, VehicleState, ViolationKind, PHYSICS_DT, WORLD_ENTITY};

use crate::course::{v2, Course, Gate, GridSlot};
use crate::error::EntryDenied;
use crate::kind::{EventId, RaceKind};
use crate::result::{
    rank, DnfReason, DqReason, DragStats, RacerResult, RacerStatus, SUBTICKS_PER_TICK,
};
use crate::rules::RaceRules;
use crate::signals::RaceSignal;
use crate::trail::Trail;

/// One racer's state as the server sees it this tick.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RacerSample {
    /// Racer vehicle entity.
    pub racer: EntityId,
    /// Chassis position, world m.
    pub pos: [f32; 3],
    /// Linear velocity, world m/s.
    pub vel: [f32; 3],
    /// Forward speed, m/s.
    pub speed_mps: f32,
    /// Throttle input 0..=1 (drag reaction time).
    pub throttle: f32,
    /// Gear (drag shift scoring).
    pub gear: i8,
    /// Engine speed, rpm (drag shift scoring).
    pub engine_rpm: f32,
}

impl RacerSample {
    /// Build a sample from a `wp_sim` vehicle state and the input applied this tick.
    pub fn from_state(racer: EntityId, s: &VehicleState, throttle: f32) -> RacerSample {
        RacerSample {
            racer,
            pos: s.pos,
            vel: s.lin_vel,
            speed_mps: s.speed_mps,
            throttle,
            gear: s.gear,
            engine_rpm: s.engine_rpm,
        }
    }
}

/// A racer entering the race.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RacerEntry {
    /// Racer vehicle entity.
    pub racer: EntityId,
    /// Engine redline, rpm (`0.0` = unknown; drag shift scoring is then skipped).
    pub redline_rpm: f32,
}

/// Race phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RacePhase {
    /// Taking entries.
    Lobby,
    /// Counting down to `green_tick`.
    Countdown {
        /// Tick of the green light.
        green_tick: u64,
    },
    /// Racing.
    Running,
    /// Over; results are final.
    Finished,
}

/// Why a race could not start.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StartError {
    /// Not enough racers.
    #[error("need at least {min} racers, have {have}")]
    NotEnoughRacers {
        /// Minimum.
        min: u8,
        /// Entered.
        have: usize,
    },
    /// Already started.
    #[error("race already started")]
    AlreadyStarted,
}

#[derive(Debug, Clone)]
struct Track {
    entry: RacerEntry,
    slot: GridSlot,
    status: RacerStatus,
    status_tick: u64,
    last: Option<(u64, [f32; 3])>,
    spawn: Option<[f32; 3]>,
    start_subtick: Option<u64>,
    next_gate: usize,
    started_lap: bool,
    gates_passed: u32,
    laps: u32,
    lap_start_subtick: u64,
    best_lap: Option<u64>,
    elapsed: Option<u64>,
    seg_hint: Option<usize>,
    progress_m: f32,
    out_ticks: u64,
    contacts: u32,
    civilian_hits: u32,
    max_contact_ns: f32,
    violations: [u32; 5],
    cargo_damage_ns: f32,
    drag: DragStats,
    last_gear: i8,
    last_rpm: f32,
    last_vel: [f32; 3],
    /// Where the racer drove (Outrun only: the gap is measured along the leader's trail).
    trail: Trail,
}

impl Track {
    fn running(&self) -> bool {
        self.status == RacerStatus::Running
    }
}

/// The judge for one race.
#[derive(Debug, Clone)]
pub struct RaceJudge {
    event: EventId,
    rules: RaceRules,
    course: Course,
    phase: RacePhase,
    tracks: Vec<Track>,
    /// Entities that are police (contacts with them are not civilian hits).
    police: Vec<EntityId>,
    green_tick: u64,
    first_finish_tick: Option<u64>,
    leader: usize,
    signals: Vec<RaceSignal>,
}

fn ticks_for(s: f32) -> u64 {
    (s / PHYSICS_DT).round().max(0.0) as u64
}

fn violation_index(k: &ViolationKind) -> usize {
    match k {
        ViolationKind::Speeding { .. } => 0,
        ViolationKind::RedLight => 1,
        ViolationKind::WrongWay => 2,
        ViolationKind::Collision => 3,
        ViolationKind::Reckless => 4,
    }
}

impl RaceJudge {
    /// New race in the lobby phase.
    pub fn new(event: EventId, rules: RaceRules, course: Course) -> RaceJudge {
        RaceJudge {
            event,
            rules,
            course,
            phase: RacePhase::Lobby,
            tracks: Vec::new(),
            police: Vec::new(),
            green_tick: 0,
            first_finish_tick: None,
            leader: 0,
            signals: Vec::new(),
        }
    }

    /// Event id.
    pub fn event(&self) -> EventId {
        self.event
    }

    /// The rules in force.
    pub fn rules(&self) -> &RaceRules {
        &self.rules
    }

    /// The course.
    pub fn course(&self) -> &Course {
        &self.course
    }

    /// Current phase.
    pub fn phase(&self) -> RacePhase {
        self.phase
    }

    /// Tick of the green light (valid once started).
    pub fn green_tick(&self) -> u64 {
        self.green_tick
    }

    /// Mark entities as police (their contacts count against clean status like any contact,
    /// but are not civilian hits).
    pub fn set_police(&mut self, police: &[EntityId]) {
        self.police = police.to_vec();
        self.police.sort();
    }

    /// Enter a racer. Grid slots are assigned in entry order (slot 0 = pole); the server
    /// decides entry order (e.g. by qualifying, rep or lobby join order).
    pub fn enter(&mut self, entry: RacerEntry) -> Result<GridSlot, EntryDenied> {
        if self.phase != RacePhase::Lobby {
            return Err(EntryDenied::LobbyClosed);
        }
        if self.tracks.iter().any(|t| t.entry.racer == entry.racer) {
            return Err(EntryDenied::AlreadyEntered);
        }
        if self.tracks.len() >= self.rules.kind_rules.max_racers as usize {
            return Err(EntryDenied::Full);
        }
        let slot = self.slot_for(self.tracks.len());
        self.tracks.push(Track {
            entry,
            slot,
            status: RacerStatus::Running,
            status_tick: 0,
            last: None,
            spawn: None,
            start_subtick: None,
            next_gate: 0,
            started_lap: false,
            gates_passed: 0,
            laps: 0,
            lap_start_subtick: 0,
            best_lap: None,
            elapsed: None,
            seg_hint: None,
            progress_m: 0.0,
            out_ticks: 0,
            contacts: 0,
            civilian_hits: 0,
            max_contact_ns: 0.0,
            violations: [0; 5],
            cargo_damage_ns: 0.0,
            drag: DragStats::default(),
            last_gear: 0,
            last_rpm: 0.0,
            last_vel: [0.0; 3],
            trail: Trail::new(match self.rules.kind {
                RaceKind::Outrun { lead_m } => lead_m + 100.0,
                _ => 0.0,
            }),
        });
        Ok(slot)
    }

    /// Grid slot `i`, laid out for the race's full capacity so slots never move as racers join
    /// (side-by-side grids are centred on `max_racers` lanes).
    fn slot_for(&self, i: usize) -> GridSlot {
        let cap = (self.rules.kind_rules.max_racers as usize).max(i + 1);
        self.course.grid(self.rules.kind_rules.grid, cap)[i]
    }

    /// Remove a racer while still in the lobby.
    pub fn withdraw(&mut self, racer: EntityId) -> bool {
        if self.phase != RacePhase::Lobby {
            return false;
        }
        let before = self.tracks.len();
        self.tracks.retain(|t| t.entry.racer != racer);
        // Re-slot everyone so the grid stays packed.
        let grid = self
            .course
            .grid(self.rules.kind_rules.grid, self.tracks.len());
        for (t, s) in self.tracks.iter_mut().zip(grid) {
            t.slot = s;
        }
        before != self.tracks.len()
    }

    /// Grid slot of every entered racer, pole first. The server places cars here
    /// (`Sim::place_vehicle`) before calling [`RaceJudge::start`].
    pub fn grid(&self) -> Vec<(EntityId, GridSlot)> {
        self.tracks
            .iter()
            .map(|t| (t.entry.racer, t.slot))
            .collect()
    }

    /// Start the countdown at `tick`. Green comes `countdown_s` later.
    pub fn start(&mut self, tick: u64) -> Result<u64, StartError> {
        if self.phase != RacePhase::Lobby {
            return Err(StartError::AlreadyStarted);
        }
        let min = self.rules.kind_rules.min_racers;
        if self.tracks.len() < min as usize {
            return Err(StartError::NotEnoughRacers {
                min,
                have: self.tracks.len(),
            });
        }
        self.green_tick = tick + ticks_for(self.rules.common.countdown_s);
        self.phase = RacePhase::Countdown {
            green_tick: self.green_tick,
        };
        self.signals.push(RaceSignal::Countdown {
            event: self.event,
            green_tick: self.green_tick,
        });
        Ok(self.green_tick)
    }

    /// Laps needed to finish (0 = point to point).
    fn laps_required(&self) -> u32 {
        match self.rules.kind {
            RaceKind::Circuit { laps } => laps.max(1) as u32,
            _ if self.course.closed_loop => 1,
            _ => 0,
        }
    }

    fn index_of(&self, racer: EntityId) -> Option<usize> {
        self.tracks.iter().position(|t| t.entry.racer == racer)
    }

    /// The server moved a racer on purpose (reset to track after a crash, respawn): the next
    /// sample is measured from `pos` so the move is not judged as a teleport. Progress and gates
    /// are kept; a reset never counts as passing a gate.
    pub fn relocate(&mut self, racer: EntityId, pos: [f32; 3], tick: u64) {
        if let Some(i) = self.index_of(racer) {
            let t = &mut self.tracks[i];
            t.last = Some((tick, pos));
            t.seg_hint = None;
            t.out_ticks = 0;
        }
    }

    /// Declare a racer DNF (police capture, disconnect, retire). Ignored if they are no longer
    /// running.
    pub fn dnf(&mut self, racer: EntityId, reason: DnfReason, tick: u64) {
        if let Some(i) = self.index_of(racer) {
            self.set_status(i, RacerStatus::Dnf(reason), tick);
        }
    }

    /// Disqualify a racer. Ignored if they are no longer running.
    pub fn disqualify(&mut self, racer: EntityId, reason: DqReason, tick: u64) {
        if let Some(i) = self.index_of(racer) {
            self.set_status(i, RacerStatus::Dq(reason), tick);
        }
    }

    fn set_status(&mut self, i: usize, status: RacerStatus, tick: u64) {
        if !self.tracks[i].running() || self.phase == RacePhase::Finished {
            return;
        }
        let racer = self.tracks[i].entry.racer;
        let t = &mut self.tracks[i];
        t.status = status;
        t.status_tick = tick;
        let event = self.event;
        match status {
            RacerStatus::Dnf(reason) => self.signals.push(RaceSignal::RacerDnf {
                event,
                racer,
                reason,
            }),
            RacerStatus::Dq(reason) => self.signals.push(RaceSignal::RacerDq {
                event,
                racer,
                reason,
            }),
            RacerStatus::Finished => {
                let elapsed = t.elapsed.unwrap_or(0);
                self.first_finish_tick.get_or_insert(tick);
                self.signals.push(RaceSignal::RacerFinished {
                    event,
                    racer,
                    elapsed_subticks: elapsed,
                });
            }
            RacerStatus::Running => {}
        }
    }

    fn finish(&mut self, i: usize, at_subtick: u64, tick: u64, speed: f32) {
        let t = &mut self.tracks[i];
        let start = t
            .start_subtick
            .unwrap_or(self.green_tick * SUBTICKS_PER_TICK);
        t.elapsed = Some(at_subtick.saturating_sub(start));
        t.drag.trap_speed_mps = speed;
        self.set_status(i, RacerStatus::Finished, tick);
    }

    /// Feed one tick: samples for (some or all) racers and the tick's simulation events.
    pub fn observe(&mut self, tick: u64, samples: &[RacerSample], events: &[SimEvent]) {
        match self.phase {
            RacePhase::Lobby | RacePhase::Finished => return,
            RacePhase::Countdown { green_tick } if tick >= green_tick => {
                self.phase = RacePhase::Running;
                self.signals.push(RaceSignal::Green {
                    event: self.event,
                    tick: green_tick,
                });
                for t in &mut self.tracks {
                    if self.rules.kind != RaceKind::TimeAttack {
                        t.start_subtick = Some(green_tick * SUBTICKS_PER_TICK);
                    }
                    t.lap_start_subtick = green_tick * SUBTICKS_PER_TICK;
                }
            }
            _ => {}
        }
        let running = self.phase == RacePhase::Running;
        for e in events {
            self.on_event(tick, e);
        }
        for s in samples {
            let Some(i) = self.index_of(s.racer) else {
                continue;
            };
            if running {
                self.on_sample_running(i, tick, s);
            } else {
                self.on_sample_countdown(i, tick, s);
            }
            let t = &mut self.tracks[i];
            t.last = Some((tick, s.pos));
            t.last_gear = s.gear;
            t.last_rpm = s.engine_rpm;
            t.last_vel = s.vel;
        }
        if running {
            if matches!(self.rules.kind, RaceKind::Outrun { .. }) {
                self.update_outrun(tick);
            }
            self.check_limits(tick);
        }
    }

    fn on_event(&mut self, tick: u64, e: &SimEvent) {
        match e {
            SimEvent::Contact {
                a, b, impulse_ns, ..
            } => {
                for (me, other) in [(*a, *b), (*b, *a)] {
                    if let Some(i) = self.index_of(me) {
                        self.on_contact(i, other, *impulse_ns, tick);
                    }
                }
            }
            SimEvent::TotalLoss { vehicle } => {
                if let Some(i) = self.index_of(*vehicle) {
                    self.set_status(i, RacerStatus::Dnf(DnfReason::TotalLoss), tick);
                }
            }
            SimEvent::TrafficViolation { vehicle, kind, .. } => {
                if let Some(i) = self.index_of(*vehicle) {
                    if self.tracks[i].running() {
                        self.tracks[i].violations[violation_index(kind)] += 1;
                    }
                }
            }
            _ => {}
        }
    }

    fn on_contact(&mut self, i: usize, other: EntityId, impulse_ns: f32, tick: u64) {
        if !self.tracks[i].running() {
            return;
        }
        let c = &self.rules.common;
        let is_racer = self.index_of(other).is_some();
        let is_police = self.police.binary_search(&other).is_ok();
        let civilian = other != WORLD_ENTITY && !is_racer && !is_police;
        let t = &mut self.tracks[i];
        t.max_contact_ns = t.max_contact_ns.max(impulse_ns);
        if civilian {
            if impulse_ns > c.civilian_hit_threshold_ns {
                t.civilian_hits += 1;
            }
        } else if impulse_ns > c.contact_clean_threshold_ns {
            t.contacts += 1;
        }
        if matches!(self.rules.kind, RaceKind::Delivery { .. }) {
            let d = &self.rules.delivery;
            if impulse_ns > d.cargo_threshold_ns {
                t.cargo_damage_ns += impulse_ns - d.cargo_threshold_ns;
                if t.cargo_damage_ns >= d.cargo_fragility_ns {
                    self.set_status(i, RacerStatus::Dnf(DnfReason::CargoDestroyed), tick);
                }
            }
        }
    }

    fn on_sample_countdown(&mut self, i: usize, tick: u64, s: &RacerSample) {
        let t = &mut self.tracks[i];
        let spawn = *t.spawn.get_or_insert(s.pos);
        if self.rules.kind == RaceKind::Drag {
            let moved = v2::len(v2::sub(v2::xy(s.pos), v2::xy(spawn)));
            if moved > self.rules.drag.jump_start_m {
                self.set_status(i, RacerStatus::Dq(DqReason::JumpStart), tick);
            }
        }
    }

    fn crossing_subtick(prev_tick: u64, tick: u64, frac: f32) -> u64 {
        let span = (tick - prev_tick) * SUBTICKS_PER_TICK;
        prev_tick * SUBTICKS_PER_TICK + (frac * span as f32).round() as u64
    }

    fn on_sample_running(&mut self, i: usize, tick: u64, s: &RacerSample) {
        if !self.tracks[i].running() {
            return;
        }
        let Some((prev_tick, prev_pos)) = self.tracks[i].last else {
            return;
        };
        if tick <= prev_tick {
            return;
        }
        let dt = (tick - prev_tick) as f32 * PHYSICS_DT;
        let moved = v2::len(v2::sub(v2::xy(s.pos), v2::xy(prev_pos)));
        if moved > self.rules.common.max_plausible_speed_mps * dt + 1.0 {
            self.set_status(i, RacerStatus::Dq(DqReason::ImplausibleMovement), tick);
            return;
        }
        if self.rules.kind == RaceKind::Drag {
            self.drag_sample(i, tick, s);
        }
        if !matches!(self.rules.kind, RaceKind::Outrun { .. }) {
            self.gates_sample(i, prev_tick, prev_pos, tick, s);
            if !self.tracks[i].running() {
                return;
            }
        }
        // Progress and corridor.
        let proj = self.course.project(s.pos, self.tracks[i].seg_hint);
        let len = self.course.length_m();
        let t = &mut self.tracks[i];
        t.seg_hint = Some(proj.segment);
        t.progress_m = if self.course.closed_loop {
            let lap_base = t.laps as f32 * len;
            if !t.started_lap && proj.along_m > len * 0.5 {
                proj.along_m - len
            } else {
                lap_base + proj.along_m
            }
        } else {
            proj.along_m
        };
        if self.rules.kind.uses_corridor() {
            let limit = self.course.corridor_half_width_m + self.rules.common.corridor_margin_m;
            if proj.off_m > limit {
                t.out_ticks += tick - prev_tick;
                if t.out_ticks > ticks_for(self.rules.common.corridor_grace_s) {
                    self.set_status(i, RacerStatus::Dq(DqReason::LeftCorridor), tick);
                }
            } else {
                t.out_ticks = 0;
            }
        }
    }

    fn drag_sample(&mut self, i: usize, tick: u64, s: &RacerSample) {
        let window = self.rules.drag.shift_window;
        let launch = self.rules.drag.launch_throttle;
        let green = self.green_tick;
        let t = &mut self.tracks[i];
        if t.drag.reaction_subticks.is_none() && s.throttle >= launch {
            t.drag.reaction_subticks = Some((tick - green) * SUBTICKS_PER_TICK);
        }
        if s.gear > t.last_gear && t.last_gear >= 1 {
            t.drag.shifts += 1;
            let red = t.entry.redline_rpm;
            if red > 0.0 && t.last_rpm >= window[0] * red && t.last_rpm <= window[1] * red {
                t.drag.good_shifts += 1;
            }
        }
    }

    fn gates_sample(
        &mut self,
        i: usize,
        prev_tick: u64,
        prev_pos: [f32; 3],
        tick: u64,
        s: &RacerSample,
    ) {
        let n = self.course.gates.len();
        if n == 0 {
            return;
        }
        let tol = self.rules.gate_tolerance_m(s.speed_mps);
        let closed = self.course.closed_loop;
        let laps_required = self.laps_required();
        // Time attack: the racer's own clock starts at the start line.
        if self.tracks[i].start_subtick.is_none() {
            let start_gate = if closed {
                self.course.gates[0]
            } else {
                let (p, d) = self.course.start();
                Gate {
                    pos: p,
                    dir: d,
                    width_m: self.course.gates[0].width_m,
                }
            };
            match start_gate.crossing(prev_pos, s.pos, tol) {
                Some(c) => {
                    let sub = Self::crossing_subtick(prev_tick, tick, c.frac);
                    let t = &mut self.tracks[i];
                    t.start_subtick = Some(sub);
                    t.lap_start_subtick = sub;
                    if closed {
                        t.started_lap = true;
                        t.next_gate = 1 % n;
                    }
                }
                None => return,
            }
        }
        // Several gates may be crossed in one coarse sample; take them in order.
        for _ in 0..n {
            let g = self.tracks[i].next_gate;
            let Some(c) = self.course.gates[g].crossing(prev_pos, s.pos, tol) else {
                break;
            };
            let sub = Self::crossing_subtick(prev_tick, tick, c.frac);
            let event = self.event;
            let racer = self.tracks[i].entry.racer;
            let t = &mut self.tracks[i];
            if closed && g == 0 && !t.started_lap {
                // Mass start: the first pass of the start/finish line opens lap 1.
                t.started_lap = true;
                t.next_gate = 1 % n;
                continue;
            }
            t.gates_passed += 1;
            let lap = t.laps + 1;
            self.signals.push(RaceSignal::CheckpointPassed {
                event,
                racer,
                gate: g as u32,
                lap,
                tick,
            });
            let t = &mut self.tracks[i];
            if closed {
                if g == 0 {
                    t.laps += 1;
                    let lap_time = sub.saturating_sub(t.lap_start_subtick);
                    t.best_lap = Some(t.best_lap.map_or(lap_time, |b| b.min(lap_time)));
                    t.lap_start_subtick = sub;
                    let laps = t.laps;
                    self.signals.push(RaceSignal::LapCompleted {
                        event,
                        racer,
                        lap: laps,
                        lap_subticks: lap_time,
                    });
                    if laps >= laps_required {
                        self.finish(i, sub, tick, s.speed_mps);
                        return;
                    }
                }
                self.tracks[i].next_gate = (g + 1) % n;
            } else if g + 1 == n {
                self.finish(i, sub, tick, s.speed_mps);
                return;
            } else {
                t.next_gate = g + 1;
            }
        }
    }

    fn update_outrun(&mut self, tick: u64) {
        let RaceKind::Outrun { lead_m } = self.rules.kind else {
            return;
        };
        if self.tracks.len() != 2 || !(self.tracks[0].running() && self.tracks[1].running()) {
            return;
        }
        let (Some((_, pl)), Some((_, pc))) = (
            self.tracks[self.leader].last,
            self.tracks[1 - self.leader].last,
        ) else {
            return;
        };
        // Leader heading: its velocity, or +Y of the course start if it is stationary.
        let vl = self.tracks[self.leader].last_vel;
        let heading = if v2::len(v2::xy(vl)) > 0.5 {
            v2::norm(v2::xy(vl))
        } else {
            self.course.start().1
        };
        self.tracks[self.leader].trail.push(v2::xy(pl));
        self.tracks[1 - self.leader].trail.push(v2::xy(pc));
        let rel = v2::sub(v2::xy(pc), v2::xy(pl));
        if v2::dot(rel, heading) > self.rules.outrun.overtake_margin_m {
            self.leader = 1 - self.leader;
            self.signals.push(RaceSignal::LeadChanged {
                event: self.event,
                leader: self.tracks[self.leader].entry.racer,
            });
            return;
        }
        // Gap along the road the leader drove (straight line if the chaser left it).
        let gap = self.tracks[self.leader]
            .trail
            .gap_m(v2::xy(pl), v2::xy(pc));
        if gap >= lead_m {
            self.outrun_decide(tick);
        }
    }

    fn outrun_decide(&mut self, tick: u64) {
        let w = self.leader;
        let l = 1 - w;
        self.finish(w, tick * SUBTICKS_PER_TICK, tick, 0.0);
        self.set_status(l, RacerStatus::Dnf(DnfReason::Outrun), tick);
    }

    fn check_limits(&mut self, tick: u64) {
        let kr = &self.rules.kind_rules;
        let time_up = tick >= self.green_tick + ticks_for(kr.time_limit_s);
        let window_up = kr.finish_window_s > 0.0
            && self
                .first_finish_tick
                .is_some_and(|f| tick >= f + ticks_for(kr.finish_window_s));
        if matches!(self.rules.kind, RaceKind::Outrun { .. }) && self.tracks.len() == 2 {
            let (a, b) = (self.tracks[0].running(), self.tracks[1].running());
            if a && b && time_up {
                self.outrun_decide(tick);
            } else if a != b {
                // The opponent crashed out / was caught / DQ'd: the survivor wins.
                let w = if a { 0 } else { 1 };
                self.finish(w, tick * SUBTICKS_PER_TICK, tick, 0.0);
            }
        } else if time_up || window_up {
            let reason = if time_up {
                DnfReason::TimeLimit
            } else {
                DnfReason::FinishWindow
            };
            for i in 0..self.tracks.len() {
                self.set_status(i, RacerStatus::Dnf(reason), tick);
            }
        }
        if self.tracks.iter().all(|t| !t.running()) {
            let results = self.results();
            self.phase = RacePhase::Finished;
            self.signals.push(RaceSignal::RaceFinished {
                event: self.event,
                results,
            });
        }
    }

    /// Whether the race is over.
    pub fn is_finished(&self) -> bool {
        self.phase == RacePhase::Finished
    }

    /// Ranked results: final once [`RaceJudge::is_finished`], provisional standings before.
    pub fn results(&self) -> Vec<RacerResult> {
        let cargo_value = match self.rules.kind {
            RaceKind::Delivery { cargo_value, .. } => Some(cargo_value),
            _ => None,
        };
        let mut out: Vec<RacerResult> = self
            .tracks
            .iter()
            .map(|t| {
                let cargo_health = if cargo_value.is_some() {
                    (1.0 - t.cargo_damage_ns / self.rules.delivery.cargo_fragility_ns)
                        .clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let cargo_damage_cost =
                    cargo_value.map_or(0, |v| ((1.0 - cargo_health) * v as f32).round() as i64);
                RacerResult {
                    racer: t.entry.racer,
                    grid_slot: t.slot.index,
                    position: 0,
                    status: t.status,
                    elapsed_subticks: t.elapsed,
                    best_lap_subticks: t.best_lap,
                    gates_passed: t.gates_passed,
                    laps_completed: t.laps,
                    progress_m: t.progress_m,
                    status_tick: t.status_tick,
                    clean: t.status == RacerStatus::Finished
                        && t.contacts == 0
                        && t.civilian_hits == 0
                        && t.violations[3] == 0,
                    contacts: t.contacts,
                    civilian_hits: t.civilian_hits,
                    max_contact_ns: t.max_contact_ns,
                    violations: t.violations,
                    cargo_health,
                    cargo_damage_cost,
                    drag: (self.rules.kind == RaceKind::Drag).then_some(t.drag),
                }
            })
            .collect();
        rank(&mut out);
        out
    }

    /// Take the signals emitted since the last call.
    pub fn drain_signals(&mut self) -> Vec<RaceSignal> {
        std::mem::take(&mut self.signals)
    }
}
