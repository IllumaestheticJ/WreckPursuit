//! Fuzz/soak: random scenarios (random inputs, cars aimed at each other) run with per-tick
//! invariant checks. Every case is a plain [`Scenario`], so a failure is written out as a
//! `.ron` file that reproduces it with `simcli run`.

use std::collections::BTreeMap;
use std::f32::consts::{FRAC_PI_2, PI};

use wp_contracts::{DamageMode, VehicleInput};
use wp_math::Rng;

use crate::data::{load_car, DataSource};
use crate::run::{run_with_hook, RunOptions, TickView};
use crate::scenario::{AssistSpec, Barrier, EntitySpec, InputKey, Scenario};
use crate::RunError;

/// Most events a single tick may emit before it counts as a flood.
pub const MAX_EVENTS_PER_TICK: usize = 1000;
/// Lowest allowed chassis z, m (ground top is z = 0; slack for hubs on the ground).
pub const MIN_CHASSIS_Z_M: f32 = -0.5;
/// How far a lattice node may sit outside the rest-pose bounding box, m.
pub const NODE_BOUNDS_MARGIN_M: f32 = 0.5;

/// Fuzz parameters.
#[derive(Debug, Clone)]
pub struct FuzzConfig {
    /// Master seed; case `i` uses a seed derived from it.
    pub seed: u64,
    /// Car ids to pick from.
    pub cars: Vec<String>,
    /// Simulated seconds per case.
    pub case_seconds: f32,
    /// Most vehicles per case.
    pub max_vehicles: u32,
}

/// A failing case.
#[derive(Debug, Clone)]
pub struct FuzzFailure {
    /// Reproduction scenario.
    pub scenario: Scenario,
    /// Tick of the violation.
    pub tick: u64,
    /// What failed.
    pub message: String,
}

/// Generate case `index`. Pure function of `(cfg, index)`.
pub fn gen_case(cfg: &FuzzConfig, index: u64) -> Scenario {
    let mut rng = Rng::new(cfg.seed ^ index.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let n = 1 + rng.below(cfg.max_vehicles.max(1));
    let mut entities = Vec::new();
    let mut i = 0;
    while i < n {
        let car = cfg.cars[rng.below(cfg.cars.len() as u32) as usize].clone();
        let base = [
            rng.range_f32(-150.0, 150.0),
            rng.range_f32(-150.0, 150.0),
            0.0,
        ];
        let assists = [
            AssistSpec::Keyboard,
            AssistSpec::Gamepad,
            AssistSpec::Wheel,
            AssistSpec::Ai,
            AssistSpec::Off,
        ][rng.below(5) as usize];
        let kind = rng.below(3);
        // 0 = free roamer, 1 = head-on pair, 2 = T-bone pair (a second car aimed at the first).
        let mut spawn = vec![(base, rng.range_f32(-PI, PI))];
        if kind > 0 && i + 1 < n {
            let gap = rng.range_f32(15.0, 60.0);
            if kind == 1 {
                spawn = vec![(base, 0.0), ([base[0], base[1] + gap, 0.0], PI)];
            } else {
                spawn = vec![
                    (base, 0.0),
                    ([base[0] + gap, base[1] + gap * 0.5, 0.0], FRAC_PI_2),
                ];
            }
        }
        // Rammers start at speed so collisions happen early in the case.
        let speed = if kind > 0 {
            rng.range_f32(8.0, 30.0)
        } else {
            0.0
        };
        for (pos, yaw) in spawn {
            entities.push(EntitySpec {
                id: format!("car{i}"),
                car: car.clone(),
                pos,
                yaw,
                speed_mps: speed,
                police_heat: 0.0,
                assists: Some(assists),
                driver: Default::default(),
                inputs: random_inputs(&mut rng, cfg.case_seconds, kind > 0),
            });
            i += 1;
        }
    }
    // Sometimes a rigid wall 10-40 m ahead of a random car.
    let mut barriers = Vec::new();
    if rng.chance(0.3) {
        let e = &entities[rng.below(entities.len() as u32) as usize];
        let (s, c) = wp_math::sin_cos(e.yaw);
        let d = rng.range_f32(10.0, 40.0);
        barriers.push(Barrier {
            pos: [e.pos[0] - s * d, e.pos[1] + c * d, 0.75],
            size: [rng.range_f32(2.0, 12.0), 1.0, 1.5],
            yaw: e.yaw,
        });
    }
    Scenario {
        name: format!("fuzz_{:016x}_{index}", cfg.seed),
        barriers,
        seed: rng.next_u64(),
        city: Default::default(),
        time_of_day_h: rng.range_f32(0.0, 24.0),
        time_ratio: 10.0,
        mode: if rng.chance(0.5) {
            DamageMode::Arcade
        } else {
            DamageMode::Simulation
        },
        assists: AssistSpec::Keyboard,
        traffic_density: None,
        police_presence: None,
        traffic_cars: true,
        parked_cars: true,
        police_cars: true,
        slow: false,
        panels: false,
        systems: None,
        entities,
        duration_s: cfg.case_seconds,
        sample_hz: 10.0,
        hash_every: 240,
        expect: Vec::new(),
        expect_pending: Vec::new(),
    }
}

fn random_inputs(rng: &mut Rng, duration: f32, ram: bool) -> Vec<InputKey> {
    let mut keys = Vec::new();
    let mut t = 0.0;
    if ram {
        keys.push(InputKey {
            t_s: 0.0,
            input: VehicleInput {
                throttle: 1.0,
                ..Default::default()
            },
        });
        t = rng.range_f32(2.0, 6.0);
    }
    while t < duration {
        keys.push(InputKey {
            t_s: t,
            input: VehicleInput {
                steer: rng.range_f32(-1.0, 1.0),
                throttle: if rng.chance(0.7) { rng.next_f32() } else { 0.0 },
                brake: if rng.chance(0.2) { rng.next_f32() } else { 0.0 },
                handbrake: if rng.chance(0.1) { 1.0 } else { 0.0 },
                clutch: 0.0,
                gear_up: rng.chance(0.05),
                gear_down: rng.chance(0.05),
                horn: rng.chance(0.05),
                headlights: rng.chance(0.5),
                siren: false,
                tool_use: 0,
            },
        });
        t += rng.range_f32(0.2, 2.0);
    }
    keys
}

fn finite(v: &[f32]) -> bool {
    v.iter().all(|x| x.is_finite())
}

/// Rest-pose node bounding box per car id, inflated by [`NODE_BOUNDS_MARGIN_M`].
pub type Bounds = BTreeMap<String, ([f32; 3], [f32; 3])>;

/// Check the fuzz invariants on one tick.
pub fn check_invariants(view: &TickView, cars: &[String], bounds: &Bounds) -> Result<(), String> {
    if view.events.len() > MAX_EVENTS_PER_TICK {
        return Err(format!(
            "event flood: {} events in one tick (> {MAX_EVENTS_PER_TICK})",
            view.events.len()
        ));
    }
    for (idx, (id, s)) in view.sim.vehicle_states().iter().enumerate() {
        // City runs add police / traffic cars after the scenario's own entities.
        let name = view.names.get(idx).map_or("city vehicle", |n| n.as_str());
        let scalars = [s.engine_rpm, s.speed_mps, s.health];
        if !(finite(&s.pos)
            && finite(&s.rot)
            && finite(&s.lin_vel)
            && finite(&s.ang_vel)
            && finite(&scalars))
        {
            return Err(format!("{name}: non-finite state {s:?}"));
        }
        for (w, ws) in s.wheel_states.iter().enumerate() {
            let v = [
                ws.omega,
                ws.steer_rad,
                ws.compression_m,
                ws.slip_angle,
                ws.slip_ratio,
                ws.fz_n,
            ];
            if !finite(&v) {
                return Err(format!("{name}: non-finite wheel {w} {ws:?}"));
            }
        }
        let q = s.rot;
        let qlen = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
        if (qlen - 1.0).abs() > 1e-2 {
            return Err(format!("{name}: rotation not unit (|q|² = {qlen})"));
        }
        if s.pos[2] < MIN_CHASSIS_Z_M {
            return Err(format!("{name}: below ground (z = {} m)", s.pos[2]));
        }
        let (lo, hi) = bounds[&cars[idx]];
        let nodes = view.sim.node_positions(*id).map_err(|e| e.to_string())?;
        for (n, p) in nodes.iter().enumerate() {
            if !finite(p) {
                return Err(format!("{name}: node {n} non-finite {p:?}"));
            }
            if (0..3).any(|k| p[k] < lo[k] || p[k] > hi[k]) {
                return Err(format!(
                    "{name}: node {n} at {p:?} escaped chassis bounds {lo:?}..{hi:?}"
                ));
            }
        }
    }
    Ok(())
}

/// Run one fuzz case. `Ok(ticks)` or the failure.
pub fn run_case(sc: &Scenario, data: &dyn DataSource) -> Result<u64, Box<FuzzFailure>> {
    let fail = |tick: u64, message: String| {
        Box::new(FuzzFailure {
            scenario: sc.clone(),
            tick,
            message,
        })
    };
    let mut bounds = Bounds::new();
    for e in &sc.entities {
        if !bounds.contains_key(&e.car) {
            let car = load_car(data, &e.car).map_err(|err| fail(0, err.to_string()))?;
            let mut lo = [f32::INFINITY; 3];
            let mut hi = [f32::NEG_INFINITY; 3];
            for n in &car.lattice.nodes {
                for k in 0..3 {
                    lo[k] = lo[k].min(n.pos_m[k] - NODE_BOUNDS_MARGIN_M);
                    hi[k] = hi[k].max(n.pos_m[k] + NODE_BOUNDS_MARGIN_M);
                }
            }
            bounds.insert(e.car.clone(), (lo, hi));
        }
    }
    let cars: Vec<String> = sc.entities.iter().map(|e| e.car.clone()).collect();
    let mut hook = |v: &TickView| check_invariants(v, &cars, &bounds);
    match run_with_hook(sc, data, &RunOptions::default(), &mut hook) {
        Ok(out) => Ok(out.summary.ticks),
        Err(RunError::Invariant { tick, message }) => Err(fail(tick, message)),
        Err(e) => Err(fail(0, e.to_string())),
    }
}

/// Run cases `0, 1, 2, …` while `keep_going(cases_done)` is true. Returns cases and ticks run,
/// or the first failure.
pub fn fuzz(
    cfg: &FuzzConfig,
    data: &dyn DataSource,
    keep_going: &mut dyn FnMut(u64) -> bool,
) -> Result<(u64, u64), Box<FuzzFailure>> {
    let mut cases = 0;
    let mut ticks = 0;
    while keep_going(cases) {
        ticks += run_case(&gen_case(cfg, cases), data)?;
        cases += 1;
    }
    Ok((cases, ticks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FsData;

    fn cfg() -> FuzzConfig {
        FuzzConfig {
            seed: 99,
            cars: vec!["debug_sedan".into(), "debug_coupe".into()],
            case_seconds: 3.0,
            max_vehicles: 6,
        }
    }

    #[test]
    fn cases_are_reproducible_and_valid() {
        let a = gen_case(&cfg(), 3);
        assert_eq!(a, gen_case(&cfg(), 3));
        assert_ne!(a, gen_case(&cfg(), 4));
        a.validate().unwrap();
        // Round-trips through RON so failures can be written as repro files.
        assert_eq!(Scenario::from_ron("x", &a.to_ron()).unwrap(), a);
    }

    #[test]
    fn short_fuzz_passes() {
        let data = FsData::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data"));
        let (cases, ticks) = fuzz(&cfg(), &data, &mut |n| n < 4)
            .map_err(|f| f.message)
            .unwrap();
        assert_eq!(cases, 4);
        assert_eq!(ticks, 4 * 720);
    }

    #[test]
    fn invariant_violation_is_reported() {
        let data = FsData::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data"));
        let mut sc = gen_case(&cfg(), 0);
        sc.entities[0].pos[2] = -5.0;
        let f = run_case(&sc, &data).unwrap_err();
        assert!(f.message.contains("below ground"), "{}", f.message);
        assert_eq!(f.tick, 1);
    }
}
