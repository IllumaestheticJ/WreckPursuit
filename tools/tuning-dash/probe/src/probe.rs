//! One sweep point: apply overrides, run one measurement, return flat metrics, series,
//! checks and artifacts as JSON ([`PointResult`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wp_contracts::DamageMode;
use wp_damage::barrier::{self, ImpactKind};

use crate::overrides::{self, Applied, Overlay, Override};

/// A job file: everything one probe run needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    /// Data root (default `data`, relative to the working directory).
    #[serde(default)]
    pub data: Option<PathBuf>,
    /// Overrides, applied in order.
    #[serde(default)]
    pub overrides: Vec<Override>,
    /// The measurement.
    pub probe: Probe,
    /// Folder for artifacts (simcli outputs). None = no artifacts.
    #[serde(default)]
    pub out: Option<PathBuf>,
}

/// What to measure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Probe {
    /// A simcli scenario (format v2): per-entity summary metrics + expectations.
    Scenario {
        /// `.ron` file.
        file: PathBuf,
        /// Systems override (simcli `--systems`).
        #[serde(default)]
        systems: Option<String>,
    },
    /// `wp_vehicle::metrics` manoeuvres on one car.
    VehicleMetrics {
        /// Car id.
        car: String,
        /// Which manoeuvres (default: all but `top_speed`): `zero_to_100`, `braking`,
        /// `skidpad`, `step_steer`, `lift_off`, `power_on`, `power_oversteer`, `top_speed`.
        #[serde(default)]
        metrics: Option<Vec<String>>,
        /// Skidpad radius, m (default 50).
        #[serde(default)]
        skidpad_radius_m: Option<f32>,
        /// Lift-off / power-on circle radius, m (default 40).
        #[serde(default)]
        turn_radius_m: Option<f32>,
    },
    /// `wp_damage` barrier harness: one car, mode and impact at several speeds.
    Barrier {
        /// Car id.
        car: String,
        /// Damage mode (`Arcade` | `Simulation`).
        mode: DamageMode,
        /// `Frontal` | `Offset` | `Side` | `Rear`.
        impact: ImpactKind,
        /// Closing speeds, km/h (default 15, 25, 40, 56, 80).
        #[serde(default)]
        speeds_kmh: Option<Vec<f32>>,
        /// Other body mass, kg (default: the car's own mass for `Side`, rigid barrier otherwise).
        #[serde(default)]
        other_mass_kg: Option<f32>,
        /// Simulated time after first contact, s (default 0.5).
        #[serde(default)]
        duration_s: Option<f32>,
        /// Damage-model seed (default 1).
        #[serde(default)]
        seed: Option<u64>,
    },
    /// A `wp_damage` barrier scenario file (`crates/wp_damage/scenarios/*.ron`) with its checks.
    BarrierScenario {
        /// `.ron` file.
        file: PathBuf,
    },
}

/// One pass/fail check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Check {
    /// What was checked.
    pub name: String,
    /// Whether it held.
    pub passed: bool,
    /// Measured value, human-readable.
    pub actual: String,
    /// Reported only, never fails the point (simcli `expect_pending`).
    pub pending: bool,
}

/// Metrics against an inner variable (barrier: closing speed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Series {
    /// Name of the x variable.
    pub x_name: String,
    /// X values.
    pub x: Vec<f64>,
    /// Metric name → one value per x (null when missing).
    pub y: BTreeMap<String, Vec<Option<f64>>>,
}

/// Probe output (`result.json`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct PointResult {
    /// False when the point could not run (`error` says why).
    pub ok: bool,
    /// Error message.
    pub error: Option<String>,
    /// Concrete changes the overrides made.
    pub applied: Vec<Applied>,
    /// Flat metrics (what sweep plots use).
    pub metrics: BTreeMap<String, Option<f64>>,
    /// Inner series (barrier speeds).
    pub series: Option<Series>,
    /// Checks (scenario expectations, barrier scenario checks).
    pub checks: Vec<Check>,
    /// All non-pending checks held (None when the probe has no checks).
    pub passed: Option<bool>,
    /// Extra facts (hashes, failed components, warnings).
    pub info: BTreeMap<String, Value>,
    /// Top-down path per entity for thumbnails (scenario probes with an `out` folder).
    pub thumb: BTreeMap<String, Vec<[f32; 2]>>,
    /// Artifact files, relative to the job's `out` folder.
    pub artifacts: Vec<String>,
}

/// Default barrier speeds, km/h (prompt 03's calibration speeds).
pub const DEFAULT_SPEEDS_KMH: [f32; 5] = [15.0, 25.0, 40.0, 56.0, 80.0];

/// Barrier metrics kept per speed.
pub const BARRIER_METRICS: [&str; 10] = [
    "crush_m",
    "cabin_intrusion_m",
    "plastic_energy_kj",
    "health",
    "broken_beams",
    "detached_parts",
    "failed_components",
    "repair_total",
    "total_loss",
    "power_scale",
];

/// Vehicle manoeuvres run by default.
pub const DEFAULT_VEHICLE_METRICS: [&str; 7] = [
    "zero_to_100",
    "braking",
    "skidpad",
    "step_steer",
    "lift_off",
    "power_on",
    "power_oversteer",
];

/// Run a job. Never panics on bad input: errors come back as `ok: false`.
pub fn run_job(job: &Job, default_data: &Path) -> PointResult {
    let root = job
        .data
        .clone()
        .unwrap_or_else(|| default_data.to_path_buf());
    match run_inner(job, &root) {
        Ok(r) => r,
        Err(e) => PointResult {
            ok: false,
            error: Some(e),
            ..Default::default()
        },
    }
}

fn f(x: f32) -> Option<f64> {
    x.is_finite().then_some(x as f64)
}

fn run_inner(job: &Job, root: &Path) -> Result<PointResult, String> {
    let mut data = Overlay::new(root);
    let mut res = PointResult {
        ok: true,
        ..Default::default()
    };
    match &job.probe {
        Probe::Scenario { file, systems } => {
            let sc = simcli::load_scenario(file).map_err(|e| e.to_string())?;
            let mut sc_val = serde_json::to_value(&sc).map_err(|e| e.to_string())?;
            res.applied = overrides::apply(&mut data, Some(&mut sc_val), &job.overrides)?;
            let sc: simcli::Scenario = serde_json::from_value(sc_val)
                .map_err(|e| format!("scenario after overrides: {e}"))?;
            sc.validate()
                .map_err(|e| format!("scenario after overrides: {e}"))?;
            let opts = simcli::RunOptions {
                systems: systems
                    .as_deref()
                    .map(simcli::Selection::parse)
                    .transpose()?,
                record: job.out.is_some(),
                ..Default::default()
            };
            let out = simcli::run(&sc, &data, &opts).map_err(|e| e.to_string())?;
            scenario_result(&mut res, &out);
            if let Some(dir) = &job.out {
                let written =
                    simcli::output::write_outputs(dir, &out).map_err(|e| e.to_string())?;
                let rel = written
                    .strip_prefix(dir)
                    .unwrap_or(&written)
                    .to_string_lossy()
                    .replace('\\', "/");
                for name in simcli::output::OUTPUT_FILES {
                    res.artifacts.push(format!("{rel}/{name}"));
                }
                res.thumb = thumbnails(&out.metrics_csv, 160);
            }
        }
        Probe::VehicleMetrics {
            car,
            metrics,
            skidpad_radius_m,
            turn_radius_m,
        } => {
            res.applied = overrides::apply(&mut data, None, &job.overrides)?;
            let car = simcli::data::load_car(&data, car).map_err(|e| e.to_string())?;
            let wanted: Vec<String> = metrics.clone().unwrap_or_else(|| {
                DEFAULT_VEHICLE_METRICS
                    .iter()
                    .map(|s| s.to_string())
                    .collect()
            });
            let skid_r = skidpad_radius_m.unwrap_or(50.0);
            let turn_r = turn_radius_m.unwrap_or(40.0);
            use wp_vehicle::metrics as vm;
            for m in &wanted {
                let put = |res: &mut PointResult, k: &str, v: Option<f64>| {
                    res.metrics.insert(k.to_string(), v);
                };
                match m.as_str() {
                    "zero_to_100" => {
                        put(&mut res, "zero_to_100_s", vm::zero_to_100(&car).and_then(f))
                    }
                    "braking" => put(&mut res, "braking_100_0_m", f(vm::braking_100_0(&car))),
                    "skidpad" => {
                        let s = vm::skidpad(&car, skid_r);
                        put(&mut res, "skidpad_max_lat_g", f(s.max_lat_g));
                        put(&mut res, "understeer_deg_per_g", f(s.understeer_deg_per_g));
                    }
                    "step_steer" => put(
                        &mut res,
                        "step_steer_overshoot_pct",
                        f(vm::step_steer(&car)),
                    ),
                    "lift_off" => {
                        let l = vm::lift_off(&car, turn_r);
                        put(&mut res, "lift_off_yaw_rate_delta_rps", f(l.yaw_rate_delta));
                        put(&mut res, "lift_off_curvature_ratio", f(l.curvature_ratio));
                    }
                    "power_on" => put(
                        &mut res,
                        "power_on_curvature_ratio",
                        f(vm::power_on(&car, turn_r)),
                    ),
                    "power_oversteer" => put(
                        &mut res,
                        "power_oversteer_throttle",
                        vm::power_oversteer_onset(&car, turn_r).and_then(f),
                    ),
                    "top_speed" => put(&mut res, "top_speed_kmh", f(vm::top_speed_kmh(&car))),
                    other => return Err(format!("unknown vehicle metric {other:?}")),
                }
            }
        }
        Probe::Barrier {
            car,
            mode,
            impact,
            speeds_kmh,
            other_mass_kg,
            duration_s,
            seed,
        } => {
            res.applied = overrides::apply(&mut data, None, &job.overrides)?;
            let car = simcli::data::load_car(&data, car).map_err(|e| e.to_string())?;
            let table = simcli::data::load_mode(&data, *mode).map_err(|e| e.to_string())?;
            let speeds = speeds_kmh.clone().unwrap_or(DEFAULT_SPEEDS_KMH.to_vec());
            let other = other_mass_kg.unwrap_or(if *impact == ImpactKind::Side {
                car.def.mass_kg
            } else {
                0.0
            });
            let mut series = Series {
                x_name: "speed_kmh".into(),
                x: speeds.iter().map(|&s| s as f64).collect(),
                y: BTreeMap::new(),
            };
            let mut failed = BTreeMap::new();
            for &speed in &speeds {
                let (_, r) = barrier::run_barrier(
                    &car,
                    &table,
                    *impact,
                    speed,
                    other,
                    duration_s.unwrap_or(0.5),
                    seed.unwrap_or(1),
                );
                for m in BARRIER_METRICS {
                    let v = r.metrics.get(m).copied().and_then(f);
                    series.y.entry(m.to_string()).or_default().push(v);
                    res.metrics.insert(format!("{m}@{}", fmt_speed(speed)), v);
                }
                failed.insert(
                    fmt_speed(speed),
                    Value::from(
                        r.failed
                            .iter()
                            .map(|c| format!("{c:?}"))
                            .collect::<Vec<_>>(),
                    ),
                );
                if !r.contained {
                    res.checks.push(Check {
                        name: format!("nodes contained @ {} km/h", fmt_speed(speed)),
                        passed: false,
                        actual: "a node left the chassis bounds".into(),
                        pending: false,
                    });
                }
            }
            res.info.insert(
                "failed_components".into(),
                Value::Object(failed.into_iter().collect()),
            );
            res.info.insert("other_mass_kg".into(), Value::from(other));
            res.series = Some(series);
            res.passed = (!res.checks.is_empty()).then_some(false);
        }
        Probe::BarrierScenario { file } => {
            res.applied = overrides::apply(&mut data, None, &job.overrides)?;
            let sc = barrier::load_scenario(file)?;
            // The barrier runner reads cars from disk: materialise the overlay in a scratch root.
            let scratch = scratch_root(&data, job.out.as_deref())?;
            let report = barrier::run_scenario(&sc, &scratch);
            let _ = std::fs::remove_dir_all(&scratch);
            let report = report?;
            let mut series = Series {
                x_name: "speed_kmh".into(),
                x: report.results.iter().map(|r| r.speed_kmh as f64).collect(),
                y: BTreeMap::new(),
            };
            for r in &report.results {
                for m in BARRIER_METRICS {
                    let v = r.metrics.get(m).copied().and_then(f);
                    series.y.entry(m.to_string()).or_default().push(v);
                    res.metrics
                        .insert(format!("{m}@{}", fmt_speed(r.speed_kmh)), v);
                }
            }
            res.series = Some(series);
            res.info
                .insert("hash".into(), Value::from(format!("{:016x}", report.hash)));
            for fl in &report.failures {
                res.checks.push(Check {
                    name: fl.clone(),
                    passed: false,
                    actual: String::new(),
                    pending: false,
                });
            }
            res.checks.insert(
                0,
                Check {
                    name: format!("{} ({} runs)", report.name, report.results.len()),
                    passed: report.failures.is_empty(),
                    actual: format!("{} failed checks", report.failures.len()),
                    pending: false,
                },
            );
            res.passed = Some(report.failures.is_empty());
        }
    }
    Ok(res)
}

/// `56` for 56.0, `12.5` for 12.5.
pub fn fmt_speed(s: f32) -> String {
    if s.fract() == 0.0 {
        format!("{}", s as i64)
    } else {
        format!("{s}")
    }
}

fn scenario_result(res: &mut PointResult, out: &simcli::RunOutput) {
    let s = &out.summary;
    for e in &s.entities {
        for m in simcli::run::EntitySummary::METRICS {
            res.metrics
                .insert(format!("{}.{m}", e.id), e.metric(m).and_then(f));
        }
    }
    for (list, pending) in [(&s.expectations, false), (&s.pending, true)] {
        for x in list {
            res.checks.push(Check {
                name: x.expect.clone(),
                passed: x.passed,
                actual: x.actual.clone(),
                pending,
            });
        }
    }
    res.passed = Some(s.passed());
    res.metrics
        .insert("passed".into(), Some(if s.passed() { 1.0 } else { 0.0 }));
    res.info
        .insert("final_hash".into(), Value::from(s.final_hash.clone()));
    res.info
        .insert("failures".into(), Value::from(s.failures.clone()));
    res.info
        .insert("warnings".into(), Value::from(s.warnings.clone()));
}

/// Down-sampled `(x, y)` path per entity from simcli `metrics.csv`.
pub fn thumbnails(csv: &str, max_points: usize) -> BTreeMap<String, Vec<[f32; 2]>> {
    let mut lines = csv.lines();
    let Some(header) = lines.next() else {
        return BTreeMap::new();
    };
    let cols: Vec<&str> = header.split(',').collect();
    let idx = |n: &str| cols.iter().position(|c| *c == n);
    let (Some(ie), Some(ix), Some(iy)) = (idx("entity"), idx("x"), idx("y")) else {
        return BTreeMap::new();
    };
    let mut all: BTreeMap<String, Vec<[f32; 2]>> = BTreeMap::new();
    for line in lines {
        let v: Vec<&str> = line.split(',').collect();
        if let (Some(e), Some(x), Some(y)) = (v.get(ie), v.get(ix), v.get(iy)) {
            if let (Ok(x), Ok(y)) = (x.parse::<f32>(), y.parse::<f32>()) {
                all.entry(e.to_string()).or_default().push([x, y]);
            }
        }
    }
    for pts in all.values_mut() {
        if pts.len() > max_points {
            let n = pts.len();
            let picked: Vec<[f32; 2]> = (0..max_points)
                .map(|i| pts[i * (n - 1) / (max_points - 1)])
                .collect();
            *pts = picked;
        }
    }
    all
}

/// Copy the data folders the barrier harness reads into a scratch root, with patches applied.
fn scratch_root(data: &Overlay, out: Option<&Path>) -> Result<PathBuf, String> {
    let base = out.map(|o| o.join(".scratch-data")).unwrap_or_else(|| {
        std::env::temp_dir().join(format!("tuning-probe-{}", std::process::id()))
    });
    let _ = std::fs::remove_dir_all(&base);
    for dir in ["cars", "tires", "lattices", "materials", "modes"] {
        let src = data.root.join(dir);
        let dst = base.join(dir);
        std::fs::create_dir_all(&dst).map_err(|e| format!("{}: {e}", dst.display()))?;
        let rd = std::fs::read_dir(&src).map_err(|e| format!("{}: {e}", src.display()))?;
        for entry in rd.flatten() {
            let p = entry.path();
            if matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("json" | "toml")
            ) {
                std::fs::copy(&p, dst.join(entry.file_name()))
                    .map_err(|e| format!("{}: {e}", p.display()))?;
            }
        }
    }
    for (rel, text) in &data.files {
        let p = base.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(base)
}
