//! Deterministic scripted race traces: racers that drive along the course centreline with a
//! simple speed model and scripted incidents. Used to simulate results without physics (tests,
//! property tests, balancing tools). Same inputs → bit-identical results on every target.

use serde::{Deserialize, Serialize};
use wp_contracts::{EntityId, SimEvent, ViolationKind, PHYSICS_DT};

use crate::course::v2;
use crate::judge::{RaceJudge, RacerEntry, RacerSample};
use crate::result::{DnfReason, RacerResult};
use crate::signals::RaceSignal;

/// Something that happens to a scripted racer at a time after green.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ScriptedIncident {
    /// Contact with another entity (`WORLD_ENTITY` for walls).
    Contact {
        /// Other entity.
        other: EntityId,
        /// Normal impulse, N·s.
        impulse_ns: f32,
    },
    /// The car is a total loss (and stops).
    TotalLoss,
    /// Police capture (the server calls `RaceJudge::dnf(.., Captured, ..)`).
    Captured,
    /// The racer quits.
    Retire,
    /// A traffic violation.
    Violation(ViolationKind),
    /// Drive offset sideways by this much from now on, m (leave the corridor).
    Swerve {
        /// New lateral offset, m.
        offset_m: f32,
    },
    /// Jump forward along the course by this much (teleport cheat), m.
    Teleport {
        /// Distance, m.
        jump_m: f32,
    },
    /// Stop driving (speed → 0).
    Stop,
}

/// A scripted racer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptedRacer {
    /// Judge entry.
    pub entry: RacerEntry,
    /// Constant acceleration until top speed, m/s².
    pub accel_mps2: f32,
    /// Top speed, m/s.
    pub top_speed_mps: f32,
    /// Launch after green, s (negative = jump start).
    pub launch_s: f32,
    /// Upshift rpm as a fraction of redline (drag shift scoring).
    pub shift_frac: f32,
    /// Incidents: `(seconds after green, what)`, ascending.
    pub incidents: Vec<(f32, ScriptedIncident)>,
}

impl ScriptedRacer {
    /// A racer with no incidents.
    pub fn new(racer: u32, accel_mps2: f32, top_speed_mps: f32) -> ScriptedRacer {
        ScriptedRacer {
            entry: RacerEntry {
                racer: EntityId(racer),
                redline_rpm: 7000.0,
            },
            accel_mps2,
            top_speed_mps,
            launch_s: 0.2,
            shift_frac: 0.93,
            incidents: Vec::new(),
        }
    }

    /// Builder: add an incident.
    pub fn with(mut self, at_s: f32, what: ScriptedIncident) -> ScriptedRacer {
        self.incidents.push((at_s, what));
        self.incidents.sort_by(|a, b| a.0.total_cmp(&b.0));
        self
    }
}

#[derive(Debug, Clone)]
struct RunState {
    along_m: f32,
    side_m: f32,
    speed: f32,
    stopped: bool,
    next_incident: usize,
    /// Current world position (continuous: lateral changes are rate-limited).
    pos: [f32; 3],
    /// Set by a teleport incident: the next position update jumps.
    teleported: bool,
}

/// Width of one scripted gear, m/s.
const GEAR_SPAN_MPS: f32 = 12.0;

/// Outcome of a simulated race.
#[derive(Debug, Clone, PartialEq)]
pub struct SimulatedRace {
    /// Final ranked results.
    pub results: Vec<RacerResult>,
    /// Every signal the judge emitted, in order.
    pub signals: Vec<RaceSignal>,
    /// Ticks simulated.
    pub ticks: u64,
}

/// Run a race with scripted racers. Racers are entered in the given order (grid order),
/// sampled every `sample_every` ticks (1 = every physics tick, 4 = every net tick) until the
/// race finishes or `max_ticks` pass.
pub fn simulate(
    mut judge: RaceJudge,
    racers: &[ScriptedRacer],
    sample_every: u64,
    max_ticks: u64,
) -> SimulatedRace {
    let sample_every = sample_every.max(1);
    let mut states = Vec::new();
    for r in racers {
        let slot = judge.enter(r.entry).expect("scripted racer entry");
        // Grid position expressed in course coordinates (along, side).
        let (p0, d) = judge.course().start();
        let rel = v2::sub(v2::xy(slot.pos), v2::xy(p0));
        states.push(RunState {
            along_m: v2::dot(rel, d),
            side_m: v2::dot(rel, v2::right(d)),
            speed: 0.0,
            stopped: false,
            next_incident: 0,
            pos: slot.pos,
            teleported: false,
        });
    }
    let green = judge.start(0).expect("scripted race start");
    let mut signals = Vec::new();
    let mut tick = 0u64;
    let mut events: Vec<SimEvent> = Vec::new();
    while tick < max_ticks && !judge.is_finished() {
        tick += 1;
        let t_rel = (tick as i64 - green as i64) as f32 * PHYSICS_DT;
        for (r, st) in racers.iter().zip(states.iter_mut()) {
            // Incidents.
            while st.next_incident < r.incidents.len() && r.incidents[st.next_incident].0 <= t_rel {
                let (_, what) = r.incidents[st.next_incident];
                st.next_incident += 1;
                match what {
                    ScriptedIncident::Contact { other, impulse_ns } => {
                        events.push(SimEvent::Contact {
                            a: r.entry.racer,
                            b: other,
                            point: [0.0; 3],
                            normal: [1.0, 0.0, 0.0],
                            impulse_ns,
                            rel_speed: impulse_ns / 1300.0,
                        })
                    }
                    ScriptedIncident::TotalLoss => {
                        st.stopped = true;
                        events.push(SimEvent::TotalLoss {
                            vehicle: r.entry.racer,
                        });
                    }
                    ScriptedIncident::Captured => {
                        st.stopped = true;
                        judge.dnf(r.entry.racer, DnfReason::Captured, tick);
                    }
                    ScriptedIncident::Retire => {
                        st.stopped = true;
                        judge.dnf(r.entry.racer, DnfReason::Retired, tick);
                    }
                    ScriptedIncident::Violation(kind) => events.push(SimEvent::TrafficViolation {
                        vehicle: r.entry.racer,
                        kind,
                    }),
                    ScriptedIncident::Swerve { offset_m } => st.side_m = offset_m,
                    ScriptedIncident::Teleport { jump_m } => {
                        st.along_m += jump_m;
                        st.teleported = true;
                    }
                    ScriptedIncident::Stop => st.stopped = true,
                }
            }
            // Speed model.
            if st.stopped {
                st.speed = (st.speed - 9.0 * PHYSICS_DT).max(0.0);
            } else if t_rel >= r.launch_s {
                st.speed = (st.speed + r.accel_mps2 * PHYSICS_DT).min(r.top_speed_mps);
            }
            st.along_m += st.speed * PHYSICS_DT;
            // Follow the centreline at the lateral offset, but never faster than the car moves
            // plus 8 m/s sideways, so corners and swerves stay physically continuous.
            let (c, d) = judge.course().point_at(st.along_m);
            let rt = v2::right(d);
            let want = [c[0] + rt[0] * st.side_m, c[1] + rt[1] * st.side_m, c[2]];
            let delta = v2::sub(v2::xy(want), v2::xy(st.pos));
            let dist = v2::len(delta);
            let max_step = (st.speed + 8.0) * PHYSICS_DT;
            if st.teleported || dist <= max_step {
                st.pos = want;
                st.teleported = false;
            } else {
                let k = max_step / dist;
                st.pos = [st.pos[0] + delta[0] * k, st.pos[1] + delta[1] * k, want[2]];
            }
        }
        if tick.is_multiple_of(sample_every) || !events.is_empty() {
            let samples: Vec<RacerSample> = racers
                .iter()
                .zip(states.iter())
                .map(|(r, st)| {
                    let (_, d) = judge.course().point_at(st.along_m);
                    let gear = 1 + (st.speed / GEAR_SPAN_MPS) as i8;
                    let in_gear = (st.speed % GEAR_SPAN_MPS) / GEAR_SPAN_MPS;
                    let red = r.entry.redline_rpm;
                    RacerSample {
                        racer: r.entry.racer,
                        pos: st.pos,
                        vel: [d[0] * st.speed, d[1] * st.speed, 0.0],
                        speed_mps: st.speed,
                        throttle: if !st.stopped && t_rel >= r.launch_s {
                            1.0
                        } else {
                            0.0
                        },
                        gear,
                        engine_rpm: red * (0.55 + (r.shift_frac - 0.55) * in_gear),
                    }
                })
                .collect();
            judge.observe(tick, &samples, &events);
            events.clear();
        }
        signals.extend(judge.drain_signals());
    }
    SimulatedRace {
        results: judge.results(),
        signals,
        ticks: tick,
    }
}
