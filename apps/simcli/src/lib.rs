//! # simcli
//!
//! Headless scenario runner (owned by workstream 10). A scenario (`.ron`, see
//! `scenarios/smoke/*.ron`) names a car, damage mode, seed, duration, scripted input
//! keyframes and expected metric ranges. [`run_scenario`] runs it and returns a
//! [`RunSummary`] with the final state hash.

#![warn(missing_docs)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use wp_contracts::{DamageMode, TimeOfDay, VehicleInput, PHYSICS_DT};
use wp_sim::{Sim, SimConfig};

/// One input keyframe: `input` applies from `t_s` until the next keyframe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputKey {
    /// Time the keyframe starts, s.
    pub t_s: f32,
    /// Input held from `t_s`.
    pub input: VehicleInput,
}

/// An expected metric range checked after the run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expect {
    /// Metric name (see [`RunSummary::metric`]).
    pub metric: String,
    /// Inclusive minimum.
    pub min: f32,
    /// Inclusive maximum.
    pub max: f32,
}

/// A headless scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    /// Scenario name.
    pub name: String,
    /// Car id (`data/cars/<car>.json`).
    pub car: String,
    /// Damage mode.
    pub mode: DamageMode,
    /// RNG seed.
    pub seed: u64,
    /// Simulated duration, s.
    pub duration_s: f32,
    /// Start time of day, hours.
    #[serde(default = "default_hour")]
    pub time_of_day_h: f32,
    /// Input keyframes, ascending by `t_s`.
    pub inputs: Vec<InputKey>,
    /// Expected ranges.
    #[serde(default)]
    pub expect: Vec<Expect>,
}

fn default_hour() -> f32 {
    12.0
}

/// Runner errors.
#[derive(Debug, Error)]
pub enum RunError {
    /// Scenario file unreadable or malformed.
    #[error("scenario {0}: {1}")]
    Scenario(PathBuf, String),
    /// Data failed to load.
    #[error(transparent)]
    Load(#[from] wp_contracts::LoadError),
    /// Sim refused an operation.
    #[error(transparent)]
    Sim(#[from] wp_sim::SimError),
}

/// Result of one run.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunSummary {
    /// Scenario name.
    pub name: String,
    /// Physics ticks run.
    pub ticks: u64,
    /// Final state hash (hex in the CLI output).
    pub state_hash: u64,
    /// Final forward speed, m/s.
    pub final_speed_mps: f32,
    /// Maximum forward speed, m/s.
    pub max_speed_mps: f32,
    /// Straight-line distance from spawn, m.
    pub distance_m: f32,
    /// Final health 0..=1.
    pub final_health: f32,
    /// Number of events emitted.
    pub events: usize,
    /// Failed expectations, human-readable. Empty = pass.
    pub failures: Vec<String>,
}

impl RunSummary {
    /// Look up a metric by name.
    pub fn metric(&self, name: &str) -> Option<f32> {
        Some(match name {
            "final_speed_mps" => self.final_speed_mps,
            "max_speed_mps" => self.max_speed_mps,
            "distance_m" => self.distance_m,
            "final_health" => self.final_health,
            _ => return None,
        })
    }
}

/// Parse a scenario file.
pub fn load_scenario(path: &Path) -> Result<Scenario, RunError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| RunError::Scenario(path.to_path_buf(), e.to_string()))?;
    ron::from_str(&text).map_err(|e| RunError::Scenario(path.to_path_buf(), e.to_string()))
}

/// Run a scenario against the data root (normally `data/`).
pub fn run_scenario(sc: &Scenario, data_root: &Path) -> Result<RunSummary, RunError> {
    let car = wp_contracts::load_car(data_root.join("cars").join(format!("{}.json", sc.car)))?;
    let mode_file = match sc.mode {
        DamageMode::Arcade => "arcade.toml",
        DamageMode::Simulation => "sim.toml",
    };
    let mode = wp_contracts::load_mode_table(data_root.join("modes").join(mode_file))?;
    let mut sim = Sim::new(SimConfig {
        seed: sc.seed,
        mode,
        time_of_day: TimeOfDay::from_hours(sc.time_of_day_h),
        time_ratio: 10.0,
    });
    let id = sim.spawn_vehicle(&car, [0.0; 3], 0.0)?;
    let ticks = (sc.duration_s / PHYSICS_DT).round() as u64;
    let mut next_key = 0;
    let mut max_speed: f32 = 0.0;
    let mut events = 0;
    for tick in 0..ticks {
        let t = tick as f32 * PHYSICS_DT;
        while next_key < sc.inputs.len() && sc.inputs[next_key].t_s <= t {
            sim.set_input(id, sc.inputs[next_key].input)?;
            next_key += 1;
        }
        sim.step_fixed();
        events += sim.drain_events().len();
        max_speed = max_speed.max(sim.vehicle_states()[0].1.speed_mps);
    }
    let state = sim.vehicle_states()[0].1.clone();
    let mut summary = RunSummary {
        name: sc.name.clone(),
        ticks,
        state_hash: sim.state_hash(),
        final_speed_mps: state.speed_mps,
        max_speed_mps: max_speed,
        distance_m: (state.pos[0] * state.pos[0] + state.pos[1] * state.pos[1]).sqrt(),
        final_health: state.health,
        events,
        failures: Vec::new(),
    };
    for e in &sc.expect {
        match summary.metric(&e.metric) {
            Some(v) if v >= e.min && v <= e.max => {}
            Some(v) => summary
                .failures
                .push(format!("{} = {v} not in [{}, {}]", e.metric, e.min, e.max)),
            None => summary
                .failures
                .push(format!("unknown metric {}", e.metric)),
        }
    }
    Ok(summary)
}
