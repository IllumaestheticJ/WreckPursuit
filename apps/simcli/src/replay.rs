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
//! tick hashes  u64 count, then per tick (state after tick i+1): u64 state hash
//! checkpoints  u32 count, then per checkpoint: u64 tick,
//!                N × (u64 entity hash, f32×3 pos, f32×3 lin_vel, f32 health)
//! ```
//!
//! The testbed (prompt 04) records the same format: setup scenario + the inputs it applied.

use wp_contracts::{VehicleInput, PHYSICS_DT};

use crate::data::DataSource;
use crate::run::{run, RunOptions, Selection};
use crate::scenario::Scenario;
use crate::RunError;

/// Current `replay.bin` format version.
pub const REPLAY_FORMAT_VERSION: u32 = 1;
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
    /// State hash after every tick.
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
        if format != REPLAY_FORMAT_VERSION {
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
    let first = golden
        .tick_hashes
        .iter()
        .zip(&now.tick_hashes)
        .position(|(a, b)| a != b);
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
    fn rejects_truncated_and_bad_magic() {
        let bytes = sample().encode();
        assert!(Replay::decode(&bytes[..bytes.len() - 1]).is_err());
        let mut bad = bytes.clone();
        bad[0] = b'X';
        assert!(Replay::decode(&bad).is_err());
    }
}
