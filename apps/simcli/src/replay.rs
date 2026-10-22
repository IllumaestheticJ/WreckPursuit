//! `replay.bin`: a self-contained, versioned binary recording of a run (setup + input stream +
//! per-tick hashes + periodic state checkpoints) and the verifier that replays it and reports
//! the first divergence.
//!
//! Layout (all integers/floats little-endian, `str` = `u32` byte length + UTF-8):
//!
//! ```text
//! magic        8 bytes  "WPREPLAY"
//! format       u32      REPLAY_FORMAT_VERSION
//! contract     str      wp_contracts::CONTRACT_VERSION at record time
//! hash_spec    u32      wp_sim::HASH_SPEC_VERSION at record time
//! systems      str      e.g. "world,vehicle,damage,police,net"
//! scenario     str      scenario RON (setup; its keyframes are ignored on replay)
//! ticks        u64
//! entities     u32      N
//! inputs       u32 count, then per event:
//!                u64 tick, u16 entity index, f32 steer, throttle, brake, handbrake, clutch,
//!                u8 flags (1 gear_up, 2 gear_down, 4 horn, 8 headlights, 16 siren), u8 tool_use
//! ops (v2+)    u32 count, then per op: u64 tick, u16 entity index, u8 kind, payload:
//!                0 reset: –
//!                1 assists: u8 flags (1 abs, 2 tcs, 4 esc, 8 auto_gears, 16 auto_clutch,
//!                  32 abs is Half, 64 tcs is Half; abs / tcs set without their Half bit =
//!                  Full, so files from before the levels read as Full / Off),
//!                  f32 steer_speed_sensitivity, countersteer_help, throttle_filter, brake_filter
//!                2 mode: u8 (0 Arcade, 1 Simulation)
//!                3 repair: –
//!                4 reset at: f32 x, y, yaw, near_z, u8 repair (0/1)
//!                5 ghost: u16 ticks (0 = solid again)
//!              Ops apply before the inputs of the same tick, in file order.
//! tick hashes  u64 count, then per tick (state after tick i+1): u64 state hash
//! checkpoints  u32 count, then per checkpoint: u64 tick,
//!                N × (u64 entity hash, f32×3 pos, f32×3 lin_vel, f32 health)
//! ```
//!
//! The testbed (prompt 04) records the same format: setup scenario + the inputs and ops it
//! applied. Version 1 files (no ops section) still decode.

use wp_contracts::{AbsLevel, AssistProfile, DamageMode, TcsLevel, VehicleInput, PHYSICS_DT};

use crate::data::DataSource;
use crate::run::{run, RunOptions, Selection};
use crate::scenario::Scenario;
use crate::RunError;

/// Current `replay.bin` format version (v2 added the ops section).
pub const REPLAY_FORMAT_VERSION: u32 = 2;
const MAGIC: &[u8; 8] = b"WPREPLAY";

/// An input change applied before a tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputEvent {
    /// Tick index the input applies from (applied before stepping tick `tick` → `tick + 1`).
    pub tick: u64,
    /// Entity index in spawn order.
    pub entity: u16,
    /// The input.
    pub input: VehicleInput,
}

/// A non-input change recorded by an interactive session (the testbed).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpKind {
    /// `Sim::reset_vehicle`.
    Reset,
    /// `Sim::set_assists`.
    Assists(AssistProfile),
    /// `Sim::set_mode_table` with the mode's table.
    Mode(DamageMode),
    /// `Sim::repair_vehicle` (full repair in place, after a paid repair bill).
    Repair,
    /// Reset in place (testbed R, playtest row 65): `Sim::place_on_ground` at `xy`/`yaw` at
    /// rest (road nearest `near_z`), then `Sim::repair_vehicle` if `repair`. The testbed picks
    /// the pose (nearest clear lane); the op carries it so playback needs no road lookup.
    ResetAt {
        /// Ground position, world m.
        xy: [f32; 2],
        /// Heading, rad about +Z.
        yaw: f32,
        /// Height hint: the road nearest this z (bridges, decks).
        near_z: f32,
        /// Also repair the car fully (not in a career run).
        repair: bool,
    },
    /// `Sim::set_ghost` (pass through other cars, props and debris for `ticks`; logged right
    /// after `ResetAt` by the testbed).
    Ghost {
        /// Ghost window, physics ticks.
        ticks: u16,
    },
    /// `Sim::set_dormant` (a car out of play sleeps as a fixed body; it wakes by itself when
    /// a car comes near or touches it, or on a reset; SR-3).
    Dormant {
        /// Sleep (`true`) or wake.
        on: bool,
    },
    /// `Sim::set_light_when_far` (an AI car far from every player drives a light kinematic
    /// model; off for a street racer racing the player).
    LightWhenFar {
        /// Allowed (`true`) or not.
        on: bool,
    },
}

/// An op applied before tick `tick`'s inputs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpEvent {
    /// Tick index the op applies before.
    pub tick: u64,
    /// Entity index in spawn order (ignored for `Mode`).
    pub entity: u16,
    /// The op.
    pub op: OpKind,
}

/// Coarse per-entity state stored every `hash_every` ticks, used to size a divergence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityCheckpoint {
    /// [`wp_sim::Sim::entity_hashes`] value.
    pub hash: u64,
    /// World position, m.
    pub pos: [f32; 3],
    /// World linear velocity, m/s.
    pub lin_vel: [f32; 3],
    /// Health 0..=1.
    pub health: f32,
}

/// All entities at one tick.
#[derive(Debug, Clone, PartialEq)]
pub struct Checkpoint {
    /// Tick (state after this many ticks).
    pub tick: u64,
    /// Entities in spawn order.
    pub entities: Vec<EntityCheckpoint>,
}

/// A recorded run.
#[derive(Debug, Clone, PartialEq)]
pub struct Replay {
    /// Contract version at record time.
    pub contract_version: String,
    /// Hash spec version at record time.
    pub hash_spec: u32,
    /// Systems that were run.
    pub systems: String,
    /// Setup scenario, RON.
    pub scenario_ron: String,
    /// Ticks run.
    pub ticks: u64,
    /// Entity count.
    pub entities: u32,
    /// Input changes, ascending by tick.
    pub inputs: Vec<InputEvent>,
    /// Ops, ascending by tick (empty for runner recordings and v1 files).
    pub ops: Vec<OpEvent>,
    /// State hash after every tick; 0 = not recorded (live testbed sessions hash every 4th
    /// tick and the last; verifiers skip 0).
    pub tick_hashes: Vec<u64>,
    /// Periodic checkpoints.
    pub checkpoints: Vec<Checkpoint>,
}

impl Replay {
    /// Empty replay for a scenario about to run.
    pub fn new(sc: &Scenario, systems: &str, entities: usize) -> Self {
        Self {
            contract_version: wp_contracts::CONTRACT_VERSION.to_string(),
            hash_spec: wp_sim::HASH_SPEC_VERSION,
            systems: systems.to_string(),
            scenario_ron: sc.to_ron(),
            ticks: 0,
            entities: entities as u32,
            inputs: Vec::new(),
            ops: Vec::new(),
            tick_hashes: Vec::new(),
            checkpoints: Vec::new(),
        }
    }

    /// Encode to bytes.
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Vec::new();
        w.extend_from_slice(MAGIC);
        put_u32(&mut w, REPLAY_FORMAT_VERSION);
        put_str(&mut w, &self.contract_version);
        put_u32(&mut w, self.hash_spec);
        put_str(&mut w, &self.systems);
        put_str(&mut w, &self.scenario_ron);
        put_u64(&mut w, self.ticks);
        put_u32(&mut w, self.entities);
        put_u32(&mut w, self.inputs.len() as u32);
        for ev in &self.inputs {
            put_u64(&mut w, ev.tick);
            w.extend_from_slice(&ev.entity.to_le_bytes());
            let i = &ev.input;
            for f in [i.steer, i.throttle, i.brake, i.handbrake, i.clutch] {
                put_f32(&mut w, f);
            }
            let flags = i.gear_up as u8
                | (i.gear_down as u8) << 1
                | (i.horn as u8) << 2
                | (i.headlights as u8) << 3
                | (i.siren as u8) << 4;
            w.push(flags);
            w.push(i.tool_use);
        }
        put_u32(&mut w, self.ops.len() as u32);
        for ev in &self.ops {
            put_u64(&mut w, ev.tick);
            w.extend_from_slice(&ev.entity.to_le_bytes());
            match ev.op {
                OpKind::Reset => w.push(0),
                OpKind::Assists(p) => {
                    w.push(1);
                    w.push(
                        p.abs.is_on() as u8
                            | (p.tcs.is_on() as u8) << 1
                            | (p.esc as u8) << 2
                            | (p.auto_gears as u8) << 3
                            | (p.auto_clutch as u8) << 4
                            | ((p.abs == AbsLevel::Half) as u8) << 5
                            | ((p.tcs == TcsLevel::Half) as u8) << 6,
                    );
                    for f in [
                        p.steer_speed_sensitivity,
                        p.countersteer_help,
                        p.throttle_filter,
                        p.brake_filter,
                    ] {
                        put_f32(&mut w, f);
                    }
                }
                OpKind::Mode(m) => {
                    w.push(2);
                    w.push(match m {
                        DamageMode::Arcade => 0,
                        DamageMode::Simulation => 1,
                    });
                }
                OpKind::Repair => w.push(3),
                OpKind::ResetAt {
                    xy,
                    yaw,
                    near_z,
                    repair,
                } => {
                    w.push(4);
                    for f in [xy[0], xy[1], yaw, near_z] {
                        put_f32(&mut w, f);
                    }
                    w.push(repair as u8);
                }
                OpKind::Ghost { ticks } => {
                    w.push(5);
                    w.extend_from_slice(&ticks.to_le_bytes());
                }
                OpKind::Dormant { on } => {
                    w.push(6);
                    w.push(on as u8);
                }
                OpKind::LightWhenFar { on } => {
                    w.push(7);
                    w.push(on as u8);
                }
            }
        }
        put_u64(&mut w, self.tick_hashes.len() as u64);
        for h in &self.tick_hashes {
            put_u64(&mut w, *h);
        }
        put_u32(&mut w, self.checkpoints.len() as u32);
        for c in &self.checkpoints {
            put_u64(&mut w, c.tick);
            for e in &c.entities {
                put_u64(&mut w, e.hash);
                for f in e.pos.iter().chain(&e.lin_vel) {
                    put_f32(&mut w, *f);
                }
                put_f32(&mut w, e.health);
            }
        }
        w
    }

    /// Decode bytes produced by [`Replay::encode`].
    pub fn decode(bytes: &[u8]) -> Result<Self, RunError> {
        let mut r = Reader { b: bytes, at: 0 };
        if r.take(8)? != MAGIC {
            return Err(RunError::Replay("not a replay (bad magic)".into()));
        }
        let format = r.u32()?;
        if !(1..=REPLAY_FORMAT_VERSION).contains(&format) {
            return Err(RunError::Replay(format!(
                "replay format v{format}, this simcli reads v{REPLAY_FORMAT_VERSION}"
            )));
        }
        let contract_version = r.str()?;
        let hash_spec = r.u32()?;
        let systems = r.str()?;
        let scenario_ron = r.str()?;
        let ticks = r.u64()?;
        let entities = r.u32()?;
        let n = r.u32()? as usize;
        let mut inputs = Vec::with_capacity(n.min(1 << 20));
        for _ in 0..n {
            let tick = r.u64()?;
            let entity = u16::from_le_bytes(r.take(2)?.try_into().expect("2 bytes"));
            let steer = r.f32()?;
            let throttle = r.f32()?;
            let brake = r.f32()?;
            let handbrake = r.f32()?;
            let clutch = r.f32()?;
            let flags = r.take(1)?[0];
            let tool_use = r.take(1)?[0];
            inputs.push(InputEvent {
                tick,
                entity,
                input: VehicleInput {
                    steer,
                    throttle,
                    brake,
                    handbrake,
                    clutch,
                    gear_up: flags & 1 != 0,
                    gear_down: flags & 2 != 0,
                    horn: flags & 4 != 0,
                    headlights: flags & 8 != 0,
                    siren: flags & 16 != 0,
                    tool_use,
                },
            });
        }
        let mut ops = Vec::new();
        if format >= 2 {
            let n = r.u32()? as usize;
            for _ in 0..n {
                let tick = r.u64()?;
                let entity = u16::from_le_bytes(r.take(2)?.try_into().expect("2 bytes"));
                let op = match r.take(1)?[0] {
                    0 => OpKind::Reset,
                    1 => {
                        let flags = r.take(1)?[0];
                        OpKind::Assists(AssistProfile {
                            abs: match (flags & 1 != 0, flags & 32 != 0) {
                                (false, _) => AbsLevel::Off,
                                (true, true) => AbsLevel::Half,
                                (true, false) => AbsLevel::Full,
                            },
                            tcs: match (flags & 2 != 0, flags & 64 != 0) {
                                (false, _) => TcsLevel::Off,
                                (true, true) => TcsLevel::Half,
                                (true, false) => TcsLevel::Full,
                            },
                            esc: flags & 4 != 0,
                            auto_gears: flags & 8 != 0,
                            auto_clutch: flags & 16 != 0,
                            steer_speed_sensitivity: r.f32()?,
                            countersteer_help: r.f32()?,
                            throttle_filter: r.f32()?,
                            brake_filter: r.f32()?,
                        })
                    }
                    2 => OpKind::Mode(match r.take(1)?[0] {
                        0 => DamageMode::Arcade,
                        1 => DamageMode::Simulation,
                        m => return Err(RunError::Replay(format!("bad mode {m}"))),
                    }),
                    3 => OpKind::Repair,
                    4 => OpKind::ResetAt {
                        xy: [r.f32()?, r.f32()?],
                        yaw: r.f32()?,
                        near_z: r.f32()?,
                        repair: r.take(1)?[0] != 0,
                    },
                    5 => OpKind::Ghost {
                        ticks: u16::from_le_bytes(r.take(2)?.try_into().expect("2 bytes")),
                    },
                    6 => OpKind::Dormant {
                        on: r.take(1)?[0] != 0,
                    },
                    7 => OpKind::LightWhenFar {
                        on: r.take(1)?[0] != 0,
                    },
                    k => return Err(RunError::Replay(format!("bad op kind {k}"))),
                };
                ops.push(OpEvent { tick, entity, op });
            }
        }
        let n = r.u64()? as usize;
        let mut tick_hashes = Vec::with_capacity(n.min(1 << 20));
        for _ in 0..n {
            tick_hashes.push(r.u64()?);
        }
        let n = r.u32()? as usize;
        let mut checkpoints = Vec::with_capacity(n.min(1 << 20));
        for _ in 0..n {
            let tick = r.u64()?;
            let mut ents = Vec::with_capacity(entities as usize);
            for _ in 0..entities {
                let hash = r.u64()?;
                let mut f = [0.0f32; 7];
                for x in &mut f {
                    *x = r.f32()?;
                }
                ents.push(EntityCheckpoint {
                    hash,
                    pos: [f[0], f[1], f[2]],
                    lin_vel: [f[3], f[4], f[5]],
                    health: f[6],
                });
            }
            checkpoints.push(Checkpoint {
                tick,
                entities: ents,
            });
        }
        if r.at != bytes.len() {
            return Err(RunError::Replay(format!(
                "{} trailing bytes",
                bytes.len() - r.at
            )));
        }
        Ok(Self {
            contract_version,
            hash_spec,
            systems,
            scenario_ron,
            ticks,
            entities,
            inputs,
            ops,
            tick_hashes,
            checkpoints,
        })
    }

    /// The setup scenario.
    pub fn scenario(&self) -> Result<Scenario, RunError> {
        Scenario::from_ron("replay", &self.scenario_ron)
    }
}

/// How one entity differs at a checkpoint.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EntityDiff {
    /// Scenario-local entity name.
    pub entity: String,
    /// Checkpoint tick the magnitudes were measured at.
    pub at_tick: u64,
    /// |Δ position|, m.
    pub pos_delta_m: f32,
    /// |Δ velocity|, m/s.
    pub vel_delta_mps: f32,
    /// Δ health (now − golden).
    pub health_delta: f32,
}

/// Result of verifying a replay.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ReplayReport {
    /// Scenario name.
    pub name: String,
    /// Every tick hash matched.
    pub matched: bool,
    /// First tick whose state hash differs (state after this many ticks).
    pub first_divergent_tick: Option<u64>,
    /// Same, in seconds.
    pub first_divergent_s: Option<f32>,
    /// Entities whose hash differs at the first checkpoint at or after the divergence.
    pub divergent_entities: Vec<String>,
    /// Magnitudes at the first checkpoint at or after the divergence.
    pub first_checkpoint_diff: Vec<EntityDiff>,
    /// Magnitudes at the last checkpoint.
    pub final_diff: Vec<EntityDiff>,
    /// Version notes (contract / hash spec changed since recording).
    pub notes: Vec<String>,
}

impl ReplayReport {
    /// Human-readable multi-line report.
    pub fn to_text(&self) -> String {
        if self.matched {
            return format!("{}: replay matches ({} notes)", self.name, self.notes.len());
        }
        let mut s = format!("{}: REPLAY DIVERGED\n", self.name);
        for n in &self.notes {
            s.push_str(&format!("  note: {n}\n"));
        }
        if let (Some(t), Some(sec)) = (self.first_divergent_tick, self.first_divergent_s) {
            s.push_str(&format!(
                "  first divergent tick: {t} ({sec:.4} s); diverged entities at next checkpoint: {:?}\n",
                self.divergent_entities
            ));
        }
        for (label, diffs) in [
            ("first checkpoint", &self.first_checkpoint_diff),
            ("final", &self.final_diff),
        ] {
            for d in diffs {
                s.push_str(&format!(
                    "  {label} @ tick {}: {} |Δpos| {:.6} m, |Δvel| {:.6} m/s, Δhealth {:+.6}\n",
                    d.at_tick, d.entity, d.pos_delta_m, d.vel_delta_mps, d.health_delta
                ));
            }
        }
        s
    }
}

/// Re-run a replay's setup with its recorded inputs and compare every tick hash.
pub fn verify(golden: &Replay, data: &dyn DataSource) -> Result<ReplayReport, RunError> {
    let sc = golden.scenario()?;
    let systems = Selection::parse(&golden.systems).map_err(RunError::Replay)?;
    let out = run(
        &sc,
        data,
        &RunOptions {
            systems: Some(systems),
            record: true,
            input_track: Some(golden.inputs.clone()),
            op_track: Some(golden.ops.clone()),
            strict: false,
        },
    )?;
    Ok(compare(&sc, golden, &out.replay))
}

/// Compare a fresh recording against a golden one.
pub fn compare(sc: &Scenario, golden: &Replay, now: &Replay) -> ReplayReport {
    let mut notes = Vec::new();
    if golden.contract_version != now.contract_version {
        notes.push(format!(
            "contract changed {} -> {}",
            golden.contract_version, now.contract_version
        ));
    }
    if golden.hash_spec != now.hash_spec {
        notes.push(format!(
            "hash spec changed v{} -> v{}: every golden must be re-blessed",
            golden.hash_spec, now.hash_spec
        ));
    }
    if golden.tick_hashes.len() != now.tick_hashes.len() {
        notes.push(format!(
            "tick count changed {} -> {}",
            golden.tick_hashes.len(),
            now.tick_hashes.len()
        ));
    }
    let name = |i: usize| {
        sc.entities
            .get(i)
            .map(|e| e.id.clone())
            .unwrap_or_else(|| format!("#{i}"))
    };
    // 0 in the recording = not recorded (a live testbed session hashes every 4th tick).
    let first = golden
        .tick_hashes
        .iter()
        .zip(&now.tick_hashes)
        .position(|(a, b)| *a != 0 && a != b);
    let mut report = ReplayReport {
        name: sc.name.clone(),
        matched: first.is_none() && notes.iter().all(|n| n.starts_with("contract")),
        first_divergent_tick: first.map(|i| i as u64 + 1),
        first_divergent_s: first.map(|i| (i as u64 + 1) as f32 * PHYSICS_DT),
        divergent_entities: Vec::new(),
        first_checkpoint_diff: Vec::new(),
        final_diff: Vec::new(),
        notes,
    };
    if let Some(i) = first {
        let tick = i as u64 + 1;
        let at = |cps: &[Checkpoint]| cps.iter().find(|c| c.tick >= tick).cloned();
        if let (Some(g), Some(n)) = (at(&golden.checkpoints), at(&now.checkpoints)) {
            report.divergent_entities = (0..g.entities.len().max(n.entities.len()))
                .filter(|e| {
                    g.entities.get(*e).map(|x| x.hash) != n.entities.get(*e).map(|x| x.hash)
                })
                .map(name)
                .collect();
            report.first_checkpoint_diff = entity_diffs(&g, &n, &name);
        }
        if let (Some(g), Some(n)) = (golden.checkpoints.last(), now.checkpoints.last()) {
            report.final_diff = entity_diffs(g, n, &name);
        }
    }
    report
}

fn entity_diffs(g: &Checkpoint, n: &Checkpoint, name: &dyn Fn(usize) -> String) -> Vec<EntityDiff> {
    g.entities
        .iter()
        .zip(&n.entities)
        .enumerate()
        .filter(|(_, (a, b))| a.hash != b.hash)
        .map(|(e, (a, b))| EntityDiff {
            entity: name(e),
            at_tick: n.tick,
            pos_delta_m: len3(sub3(a.pos, b.pos)),
            vel_delta_mps: len3(sub3(a.lin_vel, b.lin_vel)),
            health_delta: b.health - a.health,
        })
        .collect()
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn len3(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn put_u32(w: &mut Vec<u8>, v: u32) {
    w.extend_from_slice(&v.to_le_bytes());
}
fn put_u64(w: &mut Vec<u8>, v: u64) {
    w.extend_from_slice(&v.to_le_bytes());
}
fn put_f32(w: &mut Vec<u8>, v: f32) {
    w.extend_from_slice(&v.to_bits().to_le_bytes());
}
fn put_str(w: &mut Vec<u8>, s: &str) {
    put_u32(w, s.len() as u32);
    w.extend_from_slice(s.as_bytes());
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], RunError> {
        let end = self
            .at
            .checked_add(n)
            .filter(|e| *e <= self.b.len())
            .ok_or_else(|| RunError::Replay(format!("truncated at byte {}", self.at)))?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, RunError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }
    fn u64(&mut self) -> Result<u64, RunError> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("8 bytes"),
        ))
    }
    fn f32(&mut self) -> Result<f32, RunError> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn str(&mut self) -> Result<String, RunError> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|_| RunError::Replay("invalid UTF-8 string".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Replay {
        Replay {
            contract_version: "0.1.0".into(),
            hash_spec: 1,
            systems: "vehicle".into(),
            scenario_ron: "()".into(),
            ticks: 2,
            entities: 1,
            inputs: vec![InputEvent {
                tick: 0,
                entity: 0,
                input: VehicleInput {
                    throttle: 0.5,
                    gear_up: true,
                    siren: true,
                    tool_use: 3,
                    ..Default::default()
                },
            }],
            ops: vec![
                OpEvent {
                    tick: 1,
                    entity: 0,
                    op: OpKind::Reset,
                },
                OpEvent {
                    tick: 1,
                    entity: 0,
                    op: OpKind::Assists(AssistProfile::gamepad_default()),
                },
                OpEvent {
                    tick: 1,
                    entity: 0,
                    op: OpKind::Assists(AssistProfile {
                        abs: AbsLevel::Half,
                        tcs: TcsLevel::Half,
                        ..AssistProfile::wheel_default()
                    }),
                },
                OpEvent {
                    tick: 2,
                    entity: 0,
                    op: OpKind::Mode(DamageMode::Simulation),
                },
                OpEvent {
                    tick: 2,
                    entity: 0,
                    op: OpKind::Repair,
                },
                OpEvent {
                    tick: 2,
                    entity: 0,
                    op: OpKind::ResetAt {
                        xy: [12.5, -3.0],
                        yaw: 1.25,
                        near_z: 4.0,
                        repair: true,
                    },
                },
                OpEvent {
                    tick: 2,
                    entity: 0,
                    op: OpKind::Ghost { ticks: 240 },
                },
                OpEvent {
                    tick: 3,
                    entity: 0,
                    op: OpKind::Dormant { on: true },
                },
                OpEvent {
                    tick: 3,
                    entity: 0,
                    op: OpKind::LightWhenFar { on: false },
                },
            ],
            tick_hashes: vec![1, 3],
            checkpoints: vec![Checkpoint {
                tick: 2,
                entities: vec![EntityCheckpoint {
                    hash: 9,
                    pos: [1.0, 2.0, 3.0],
                    lin_vel: [0.0; 3],
                    health: 0.5,
                }],
            }],
        }
    }

    #[test]
    fn encode_decode_round_trip() {
        let r = sample();
        assert_eq!(Replay::decode(&r.encode()).unwrap(), r);
    }

    #[test]
    fn legacy_assist_flags_read_as_full_or_off() {
        // A pre-level file: abs + tcs bits only (no Half bits) → Full / Full; none → Off.
        let mut r = sample();
        r.ops = vec![OpEvent {
            tick: 0,
            entity: 0,
            op: OpKind::Assists(AssistProfile::keyboard_default()),
        }];
        let bytes = r.encode();
        let back = Replay::decode(&bytes).unwrap();
        let OpKind::Assists(p) = back.ops[0].op else {
            panic!("assists op")
        };
        assert_eq!((p.abs, p.tcs), (AbsLevel::Full, TcsLevel::Full));
    }

    #[test]
    fn rejects_truncated_and_bad_magic() {
        let bytes = sample().encode();
        assert!(Replay::decode(&bytes[..bytes.len() - 1]).is_err());
        let mut bad = bytes.clone();
        bad[0] = b'X';
        assert!(Replay::decode(&bad).is_err());
    }
}
