//! Testbed scenarios.
//!
//! [`Scenario::from_ron`] accepts simcli's canonical format v2 (`apps/simcli/SCENARIOS.md`,
//! converted by [`Scenario::from_simcli`]) and the older testbed dialect used by
//! `apps/testbed-web/scenarios/`: vehicle 1 = `car`, spawned at the origin facing +Y, driven by
//! `inputs`, plus these optional fields:
//!
//! - `cruise_kmh`: hold vehicle 1 at this speed with a P-controller instead of using `inputs`
//!   (barrier tests "at chosen speed"; the browser can override it).
//! - `others`: extra scripted vehicles (`car`, `pos`, `yaw`, `inputs`, `cruise_kmh`).
//! - `barriers`: static barrier boxes. **Drawn only** until `wp_sim` exposes static geometry
//!   (contract change request TB-3).
//! - `pending_expect`: expectations that need real vehicle/damage physics (prompts 02/03/12).
//!   Shown with pass/fail in the browser but never gate `cargo test`.
//!
//! Metric names follow simcli (`final_speed_mps`, `max_speed_mps`, `distance_m`,
//! `final_health`) plus `min_health` and `peak_decel_g`; prefix `vN.` addresses vehicle N
//! (1-based, `v1.` is optional).

use serde::{Deserialize, Serialize};
use wp_contracts::{DamageMode, VehicleInput, VehicleState};

/// One input keyframe: `input` applies from `t_s` until the next keyframe (simcli format).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputKey {
    /// Time the keyframe starts, s.
    pub t_s: f32,
    /// Input held from `t_s`.
    pub input: VehicleInput,
}

/// An expected metric range checked at the end of the scenario (simcli format).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expect {
    /// Metric name, optionally prefixed `vN.`.
    pub metric: String,
    /// Inclusive minimum.
    pub min: f32,
    /// Inclusive maximum.
    pub max: f32,
}

/// A static barrier box (world space). Visual only for now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Barrier {
    /// Centre of the box, m.
    pub pos: [f32; 3],
    /// Full size `[x, y, z]`, m.
    pub size: [f32; 3],
    /// Heading about +Z, rad.
    #[serde(default)]
    pub yaw: f32,
}

/// An additional scripted vehicle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtraVehicle {
    /// Car id.
    pub car: String,
    /// Spawn position, m.
    pub pos: [f32; 3],
    /// Spawn heading about +Z, rad (0 = +Y).
    #[serde(default)]
    pub yaw: f32,
    /// Input keyframes.
    #[serde(default)]
    pub inputs: Vec<InputKey>,
    /// Speed hold, km/h (overrides `inputs` when set).
    #[serde(default)]
    pub cruise_kmh: Option<f32>,
}

fn default_hour() -> f32 {
    12.0
}

fn default_car() -> String {
    "debug_sedan".to_string()
}

fn default_name() -> String {
    "free_drive".to_string()
}

/// A testbed scenario (superset of simcli's `Scenario`). [`Scenario::default`] is free drive:
/// one unscripted `debug_sedan` for the player, Arcade, no duration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scenario {
    /// Scenario name.
    #[serde(default = "default_name")]
    pub name: String,
    /// Car id of vehicle 1.
    #[serde(default = "default_car")]
    pub car: String,
    /// Damage mode.
    #[serde(default)]
    pub mode: DamageMode,
    /// RNG seed (the `init(seed, ..)` argument overrides it when non-zero).
    #[serde(default)]
    pub seed: u64,
    /// Scripted duration, s. `0` = unscripted free drive (vehicle 1 is the player).
    #[serde(default)]
    pub duration_s: f32,
    /// Start time of day, hours.
    #[serde(default = "default_hour")]
    pub time_of_day_h: f32,
    /// Vehicle 1 input keyframes.
    #[serde(default)]
    pub inputs: Vec<InputKey>,
    /// Gating expectations (also checked by simcli).
    #[serde(default)]
    pub expect: Vec<Expect>,
    /// Testbed extension: vehicle 1 speed hold, km/h.
    #[serde(default)]
    pub cruise_kmh: Option<f32>,
    /// Testbed extension: extra scripted vehicles.
    #[serde(default)]
    pub others: Vec<ExtraVehicle>,
    /// Testbed extension: static barriers (drawn only until `wp_sim` supports them).
    #[serde(default)]
    pub barriers: Vec<Barrier>,
    /// Testbed extension: expectations awaiting real physics/damage; reported, never gating.
    #[serde(default)]
    pub pending_expect: Vec<Expect>,
}

impl Default for Scenario {
    fn default() -> Self {
        Self {
            name: default_name(),
            car: default_car(),
            mode: DamageMode::Arcade,
            seed: 1,
            duration_s: 0.0,
            time_of_day_h: default_hour(),
            inputs: Vec::new(),
            expect: Vec::new(),
            cruise_kmh: None,
            others: Vec::new(),
            barriers: Vec::new(),
            pending_expect: Vec::new(),
        }
    }
}

impl Scenario {
    /// Parse a `.ron` scenario: simcli format v2 (`entities: [...]`, the canonical format)
    /// first, then the older testbed dialect (`car` + `others`).
    pub fn from_ron(text: &str) -> Result<Self, String> {
        match simcli::scenario::Scenario::from_ron("scenario", text) {
            Ok(v2) => Ok(Self::from_simcli(&v2)),
            Err(v2_err) => ron::from_str(text).map_err(|e| {
                format!("not a simcli v2 scenario ({v2_err}) nor a testbed one ({e})")
            }),
        }
    }

    /// Convert a simcli v2 scenario (lead adapter until workstream 04 adopts v2 natively).
    /// Approximations: entity 1 spawns at the origin facing +Y; a `cruise` driver holds its
    /// speed for the whole run (its `from_s`/`until_s` window is ignored); assists use the
    /// session defaults; only `metric(...)` expectations the testbed knows are kept.
    pub fn from_simcli(v2: &simcli::scenario::Scenario) -> Self {
        use simcli::scenario::{Driver, Expect as V2Expect};
        let keys = |e: &simcli::scenario::EntitySpec| -> Vec<InputKey> {
            e.inputs
                .iter()
                .map(|k| InputKey {
                    t_s: k.t_s,
                    input: k.input,
                })
                .collect()
        };
        let cruise = |e: &simcli::scenario::EntitySpec| match e.driver {
            Driver::Cruise { target_mps, .. } => Some(target_mps * 3.6),
            _ => None,
        };
        let index_of = |name: &Option<String>| {
            name.as_ref()
                .and_then(|n| v2.entities.iter().position(|e| &e.id == n))
                .unwrap_or(0)
        };
        let metrics = |list: &[V2Expect]| -> Vec<Expect> {
            list.iter()
                .filter_map(|x| match x {
                    V2Expect::Metric {
                        name,
                        entity,
                        min,
                        max,
                    } if VehicleMetrics::new().metric(name).is_some() => Some(Expect {
                        metric: format!("v{}.{name}", index_of(entity) + 1),
                        min: *min,
                        max: *max,
                    }),
                    _ => None,
                })
                .collect()
        };
        let first = v2.entities.first();
        Self {
            name: v2.name.clone(),
            car: first.map_or_else(default_car, |e| e.car.clone()),
            mode: v2.mode,
            seed: v2.seed,
            duration_s: v2.duration_s,
            time_of_day_h: v2.time_of_day_h,
            inputs: first.map(keys).unwrap_or_default(),
            expect: metrics(&v2.expect),
            cruise_kmh: first.and_then(cruise),
            others: v2
                .entities
                .iter()
                .skip(1)
                .map(|e| ExtraVehicle {
                    car: e.car.clone(),
                    pos: e.pos,
                    yaw: e.yaw,
                    inputs: keys(e),
                    cruise_kmh: cruise(e),
                })
                .collect(),
            barriers: Vec::new(),
            pending_expect: metrics(&v2.expect_pending),
        }
    }

    /// Whether vehicles are script-driven (non-zero duration).
    pub fn is_scripted(&self) -> bool {
        self.duration_s > 0.0
    }
}

/// Per-tick input script for one scripted vehicle.
#[derive(Debug, Clone, PartialEq)]
pub struct InputScript {
    /// Keyframes, ascending by time.
    pub keys: Vec<InputKey>,
    /// Speed hold target, m/s.
    pub cruise_mps: Option<f32>,
}

impl InputScript {
    /// Input for simulation time `t` given the vehicle's current forward speed.
    /// Cruise throttle/brake is quantised to 1/256 so the replay log stays small.
    pub fn input_at(&self, t: f32, speed_mps: f32) -> VehicleInput {
        let mut input = self
            .keys
            .iter()
            .rev()
            .find(|k| k.t_s <= t)
            .map(|k| k.input)
            .unwrap_or_default();
        if let Some(target) = self.cruise_mps {
            let err = target - speed_mps;
            let q = |x: f32| (x.clamp(0.0, 1.0) * 256.0).round() / 256.0;
            input.throttle = q(err * 0.5);
            input.brake = q(-err * 0.3 - 0.3);
        }
        input
    }
}

/// Running metrics for one vehicle.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Default)]
pub struct VehicleMetrics {
    /// Final forward speed, m/s.
    pub final_speed_mps: f32,
    /// Maximum forward speed, m/s.
    pub max_speed_mps: f32,
    /// Straight-line distance from spawn (XY), m.
    pub distance_m: f32,
    /// Final health.
    pub final_health: f32,
    /// Minimum health seen.
    pub min_health: f32,
    /// Peak longitudinal deceleration, g.
    pub peak_decel_g: f32,
}

impl VehicleMetrics {
    /// Fresh metrics for a pristine vehicle at rest.
    pub fn new() -> Self {
        Self {
            final_health: 1.0,
            min_health: 1.0,
            ..Default::default()
        }
    }

    /// Fold in one tick of state. `spawn` is the spawn position, `decel_g` this tick's
    /// longitudinal deceleration in g (positive = slowing).
    pub fn update(&mut self, s: &VehicleState, spawn: [f32; 3], decel_g: f32) {
        self.final_speed_mps = s.speed_mps;
        self.max_speed_mps = self.max_speed_mps.max(s.speed_mps);
        let dx = s.pos[0] - spawn[0];
        let dy = s.pos[1] - spawn[1];
        self.distance_m = (dx * dx + dy * dy).sqrt();
        self.final_health = s.health;
        self.min_health = self.min_health.min(s.health);
        self.peak_decel_g = self.peak_decel_g.max(decel_g);
    }

    /// Look up a metric by its unprefixed name.
    pub fn metric(&self, name: &str) -> Option<f32> {
        Some(match name {
            "final_speed_mps" => self.final_speed_mps,
            "max_speed_mps" => self.max_speed_mps,
            "distance_m" => self.distance_m,
            "final_health" => self.final_health,
            "min_health" => self.min_health,
            "peak_decel_g" => self.peak_decel_g,
            _ => return None,
        })
    }
}

/// Outcome of one expectation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CheckResult {
    /// Metric name as written in the scenario.
    pub metric: String,
    /// Inclusive minimum.
    pub min: f32,
    /// Inclusive maximum.
    pub max: f32,
    /// Measured value (`None` = unknown metric / vehicle).
    pub value: Option<f32>,
    /// Whether the value is inside the range.
    pub pass: bool,
    /// From `pending_expect` (informational, needs real physics).
    pub pending: bool,
}

/// Evaluate expectations against per-vehicle metrics (index 0 = vehicle 1).
pub fn evaluate(expect: &[Expect], metrics: &[VehicleMetrics], pending: bool) -> Vec<CheckResult> {
    expect
        .iter()
        .map(|e| {
            let (vehicle, name) = split_metric(&e.metric);
            let value = metrics.get(vehicle).and_then(|m| m.metric(name));
            CheckResult {
                metric: e.metric.clone(),
                min: e.min,
                max: e.max,
                value,
                pass: value.is_some_and(|v| v >= e.min && v <= e.max),
                pending,
            }
        })
        .collect()
}

/// Split `"v2.final_health"` into `(1, "final_health")`; unprefixed names mean vehicle 1.
fn split_metric(metric: &str) -> (usize, &str) {
    if let Some(rest) = metric.strip_prefix('v') {
        if let Some((n, name)) = rest.split_once('.') {
            if let Ok(n) = n.parse::<usize>() {
                return (n.saturating_sub(1), name);
            }
        }
    }
    (0, metric)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simcli_and_testbed_scenarios_parse() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let mut stack = vec![
            std::path::Path::new(root).join("apps/simcli/scenarios"),
            std::path::Path::new(root).join("apps/testbed-web/scenarios"),
        ];
        let mut n = 0;
        while let Some(p) = stack.pop() {
            for e in std::fs::read_dir(&p).unwrap() {
                let path = e.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|x| x == "ron") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    Scenario::from_ron(&text).unwrap_or_else(|e| panic!("{path:?}: {e}"));
                    n += 1;
                }
            }
        }
        assert!(n >= 2);
    }

    #[test]
    fn metric_prefixes() {
        assert_eq!(split_metric("final_health"), (0, "final_health"));
        assert_eq!(split_metric("v2.final_health"), (1, "final_health"));
        assert_eq!(split_metric("v1.distance_m"), (0, "distance_m"));
    }

    #[test]
    fn cruise_holds_speed_band() {
        let s = InputScript {
            keys: vec![],
            cruise_mps: Some(10.0),
        };
        assert_eq!(s.input_at(0.0, 0.0).throttle, 1.0);
        assert_eq!(s.input_at(0.0, 0.0).brake, 0.0);
        assert_eq!(s.input_at(0.0, 15.0).throttle, 0.0);
        assert!(s.input_at(0.0, 15.0).brake > 0.5);
    }
}
