//! Offline training of the AI street racers (owner request 2026-10-24: "train the AI by having
//! them do a full speed run with no traffic, and then have the AI separately dodge
//! traffic/ram police while racing"). Writes `data/ai/racer_training.json`, which
//! `wp_opponent::learned` embeds and `wp_opponent::Field` applies.
//!
//! **Stage 1 (`stage1`): full-speed runs, no traffic, no police.** Each trained car alone on
//! each trained course, a flat-out, noise-free driver (skill and consistency 1), run again and
//! again. Per 10 m of road the trainer learns, by iterative learning control:
//! - the **aim correction**: the car's mean drift off its commanded line there (measured a
//!   steering response time later) is subtracted from the aim, so the car holds its line
//!   through the long sweepers instead of drifting wide and being slowed by the off-line cap;
//! - the **corner-speed scale**: up 1.5 % where the car went through clean and on its line,
//!   down 3 % over the stretch leading into a place where it lost the rear, ran wide, hit a
//!   wall or had to be reset.
//!
//! A round is kept only when the lap is faster without more wall hits or resets; otherwise it
//! is rolled back and the steps halve. Points are stored by world position and direction
//! (`wp_opponent::learned`), so Loop 9 night and day share the Loop 9 training.
//!
//! **Stage 2 (`stage2`): traffic and police.** Full fields with the race's own traffic and
//! police over several seeds; a small grid search over the learned traffic numbers
//! (`LearnedTraffic`: civilian clearance and time gap, ramming nerve and closing speed, spike
//! strip clearance) scored on finishers, civilian contacts, busts and time. The racer
//! behaviours themselves (dodging traffic, ramming police in the way, steering round spike
//! strips) live in `wp_opponent::field`; police AI is never changed.
//!
//! ```sh
//! # stage 1, then stage 2 (release; WP_TRAIN_WRITE=1 writes the table)
//! WP_TRAIN_WRITE=1 cargo test --release -p wp_race --test opponent_train stage1 -- --ignored --nocapture
//! WP_TRAIN_WRITE=1 cargo test --release -p wp_race --test opponent_train stage2 -- --ignored --nocapture
//! ```
//! Knobs: `WP_TRAIN_RACES=loop9_circuit,…`, `WP_TRAIN_CARS=kestrel_ts,…`,
//! `WP_TRAIN_ROUNDS=10`, `WP_TRAIN_SEEDS=1,2,3`, `WP_TRAIN_THREADS=4`.

use std::path::{Path, PathBuf};

use simcli::run::build_sim;
use simcli::{load_scenario, FsData, RunOptions};
use wp_contracts::{
    DamageSim, EntityId, LoadedCar, SimEvent, VehicleSim, PHYSICS_DT, WORLD_ENTITY,
};
use wp_opponent::learned::{to_hex, CarCourse, CourseGeom, LearnedCar, POINT_STEP_M};
use wp_opponent::{
    CarCapability, Corridor, DriverProfile, LearnedPath, LearnedTable, LearnedTraffic, SimPolice,
    SpeedLimit, VehicleObs, CORRIDOR_STEP_M,
};
use wp_race::catalog;
use wp_race::entrants::{AiEntrant, AiEntrants};
use wp_race::{EventId, Legality, RaceJudge, RacerEntry, RacerSample};
use wp_sim::{Sim, VehicleKind};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn table_path() -> PathBuf {
    repo().join("data/ai/racer_training.json")
}

fn obs(sim: &Sim) -> Vec<VehicleObs> {
    sim.vehicles()
        .iter()
        .map(|v| VehicleObs {
            entity: v.id,
            state: v.vehicle.state(),
            effects: v.damage.handling_effects(),
            size_m: [
                v.vehicle.def().body_size_m[0],
                v.vehicle.def().body_size_m[1],
            ],
        })
        .collect()
}

fn list(var: &str, default: &[&str]) -> Vec<String> {
    std::env::var(var)
        .ok()
        .map(|v| v.split(',').map(|x| x.trim().to_string()).collect())
        .unwrap_or_else(|| default.iter().map(|x| x.to_string()).collect())
}

fn num<T: std::str::FromStr>(var: &str, default: T) -> T {
    std::env::var(var)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Courses trained by default: the street races the game keeps (owner, 2026-10-24). Loop 9
/// day runs the same road as the night race, so its training comes from `loop9_circuit`.
/// Ids not in the catalog are skipped.
const COURSES: [&str; 2] = ["loop9_circuit", "loop9_ashpeak_highway"];

/// Cars trained by default: every car in `data/cars` but the debug cars and the police
/// units (`sentinel_*`, never AI racers). Street racer pools and race fields change between
/// branches; position-keyed training makes any of them usable anywhere on trained road.
fn default_cars() -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(repo().join("data/cars"))
        .unwrap()
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            let id = name.strip_suffix(".json")?.to_string();
            (!id.starts_with("debug_") && !id.starts_with("sentinel_")).then_some(id)
        })
        .collect();
    out.sort();
    out
}

/// Training bin length, m (stored every [`POINT_STEP_M`], two bins per stored point).
const TRAIN_BIN_M: f32 = 10.0;
/// Corridor samples per training bin.
const BIN_SAMPLES: usize = (TRAIN_BIN_M / CORRIDOR_STEP_M) as usize;
/// Training bins per stored point.
const BINS_PER_POINT: usize = (POINT_STEP_M / TRAIN_BIN_M) as usize;

/// What one solo run measured, per bin of [`TRAIN_BIN_M`] along the race corridor.
#[derive(Debug, Clone)]
struct SoloRun {
    /// Judged time, s (None = did not finish).
    time_s: Option<f32>,
    walls: u32,
    resets: u32,
    /// Σ (n − n_cmd), count, Σ v, at speed (> 15 m/s), per bin.
    err: Vec<(f32, u32, f32)>,
    /// Lost the rear, ran wide, hit a wall or was reset in this bin.
    bad: Vec<bool>,
    /// The race corridor (for positions).
    corridor: Corridor,
    /// The car's measured capability.
    cap: CarCapability,
}

fn scenario_path(id: &str) -> PathBuf {
    repo().join(format!("apps/simcli/scenarios/race/pick_{id}.ron"))
}

/// One car alone on course `id` (the pole seat), noise-free and flat out, no traffic, no
/// police, with `learned` corrections and steering look-ahead multiplier `lookahead`.
fn solo_run(id: &str, car_id: &str, learned: Option<&LearnedPath>, lookahead: f32) -> SoloRun {
    let mut sc = load_scenario(&scenario_path(id)).unwrap();
    sc.police_presence = Some(0.0);
    sc.police_cars = false;
    sc.traffic_density = Some(0.0);
    // The pole seat (entity 1) in `car_id`, and the player's (entity 0, last on the grid)
    // parked there: the judge wants two entrants.
    let mut pole = sc.entities[1].clone();
    pole.car = car_id.to_string();
    sc.entities = vec![pole, sc.entities[0].clone()];
    let data_src = FsData::new(repo().join("data"));
    let built = build_sim(&sc, &data_src, &RunOptions::default()).unwrap();
    let mut sim = built.sim;
    let racer = built.ids[0];
    let parked = built.ids[1];
    sim.set_player_steering(racer, Some(false)).unwrap();
    sim.set_vehicle_kind(racer, VehicleKind::AiRacer).unwrap();
    let def = catalog::race(id).unwrap();
    let race = catalog::plan_course(sim.world(), &def).unwrap();
    let car: &LoadedCar = &built.cars[car_id];
    let mut judge = RaceJudge::new(EventId(1), def.rules(), race.course.clone());
    judge
        .enter(RacerEntry {
            racer,
            redline_rpm: car.def.engine.redline_rpm,
        })
        .unwrap();
    judge
        .enter(RacerEntry {
            racer: parked,
            redline_rpm: 7000.0,
        })
        .unwrap();
    let entrant = AiEntrant {
        racer,
        profile: DriverProfile {
            skill: 1.0,
            consistency: 1.0,
            ..DriverProfile::default()
        },
        capability: CarCapability::for_car(car),
    };
    let cap = entrant.capability;
    let closed = def.legality == Legality::Sanctioned;
    let mut ai = AiEntrants::new(sim.world(), &race, def.kind, closed, &[entrant], def.seed)
        .expect("a drivable route");
    let corridor = ai.field().corridor().clone();
    let m = corridor.len();
    // A NaN look-ahead: drive with what the embedded table gives this car, as the game does.
    if !lookahead.is_nan() {
        ai.field_mut().set_learned(
            0,
            Some(learned.cloned().unwrap_or_else(|| LearnedPath::neutral(m))),
        );
        ai.field_mut().set_lookahead_scale(0, lookahead);
    }
    judge.set_stranded_reset(&[racer]);
    let green = judge.start(sim.tick()).unwrap();
    let bins = m.div_ceil(BIN_SAMPLES);
    let mut run = SoloRun {
        time_s: None,
        walls: 0,
        resets: 0,
        err: vec![(0.0, 0, 0.0); bins],
        bad: vec![false; bins],
        corridor,
        cap,
    };
    let bin_of = |s: f32| ((s / TRAIN_BIN_M).max(0.0) as usize).min(bins - 1);
    let mut events: Vec<SimEvent> = Vec::new();
    let max_s = (race.route.length_m / 8.0 + 60.0) as u64;
    while sim.tick() < green + 240 * max_s {
        let tick = sim.tick();
        let inputs = ai.tick(
            &judge,
            tick,
            sim.world(),
            &obs(&sim),
            &events,
            &SimPolice(sim.police()),
        );
        for (e, inp) in inputs {
            sim.set_input(e, inp).unwrap();
        }
        sim.set_input(
            parked,
            wp_contracts::VehicleInput {
                handbrake: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
        sim.step_fixed();
        events = sim.drain_events();
        let samples: Vec<RacerSample> = sim
            .vehicles()
            .iter()
            .filter(|v| v.id == racer || v.id == parked)
            .map(|v| RacerSample::from_state(v.id, &v.vehicle.state(), v.input.throttle))
            .collect();
        judge.observe(sim.tick(), &samples, &events);
        ai.report_to(&mut judge, sim.tick());
        let d = ai.field().debug_of(0);
        let b = bin_of(d.s);
        for ev in &events {
            if let SimEvent::Contact {
                a,
                b: other,
                impulse_ns,
                ..
            } = ev
            {
                if *a == racer && *other == WORLD_ENTITY && *impulse_ns >= 400.0 && tick >= green {
                    run.walls += 1;
                    run.bad[b] = true;
                }
            }
        }
        for r in judge.drain_resets() {
            let yaw = wp_math::atan2(-r.dir[0], r.dir[1]);
            sim.place_on_ground(r.racer, [r.pos[0], r.pos[1]], yaw, 0.0, Some(r.pos[2]))
                .unwrap();
            sim.set_ghost(r.racer, 240).unwrap();
            judge.relocate(r.racer, r.pos, sim.tick());
            ai.relocate(r.racer, r.pos);
            run.resets += 1;
            run.bad[b] = true;
        }
        let res = judge.results();
        let me = res.iter().find(|r| r.racer == racer).unwrap();
        if me.status != wp_race::RacerStatus::Running {
            run.time_s = me
                .elapsed_subticks
                .filter(|_| me.status == wp_race::RacerStatus::Finished)
                .map(|t| t as f32 * PHYSICS_DT / wp_race::result::SUBTICKS_PER_TICK as f32);
            break;
        }
        if tick < green || !tick.is_multiple_of(4) {
            continue;
        }
        let v = sim
            .vehicles()
            .iter()
            .find(|x| x.id == racer)
            .map(|x| x.vehicle.state().speed_mps)
            .unwrap_or(0.0);
        if std::env::var("WP_TRAIN_DEBUG").is_ok()
            && tick.is_multiple_of(num("WP_TRAIN_DEBUG_TICKS", 240u64))
        {
            let th = sim
                .vehicles()
                .iter()
                .find(|x| x.id == racer)
                .map(|x| x.input.throttle)
                .unwrap_or(0.0);
            eprintln!("    t {:.0} s {:.0} v {v:.1} tgt {:.1} lim {:?} th {th:.2} n {:.2} ncmd {:.2} line {:.2} wide {}", (tick - green) as f32 / 240.0, d.s, d.v_target, d.limit, d.n, d.n_cmd, d.line_n, d.wide);
        }
        if d.limit == SpeedLimit::Unstable {
            run.bad[b] = true;
        }
        if v > 15.0
            && matches!(
                d.limit,
                SpeedLimit::Profile | SpeedLimit::Mode | SpeedLimit::Unstable
            )
        {
            let e = &mut run.err[b];
            e.0 += d.n - d.n_cmd;
            e.1 += 1;
            e.2 += v;
        }
    }
    run
}

/// Mean |drift off the commanded line| at speed, m, and bins drifting more than 1 m.
fn drift(r: &SoloRun) -> (f32, usize) {
    let (mut sum, mut n, mut big) = (0.0, 0u32, 0);
    for (e, c, _) in &r.err {
        if *c > 0 {
            sum += e.abs();
            n += c;
            if (e / *c as f32).abs() > 1.0 {
                big += 1;
            }
        }
    }
    (sum / n.max(1) as f32, big)
}

/// The learned path (per corridor sample) from per-bin values.
fn path_from_bins(m: usize, scale: &[f32], aim: &[f32], v_ref: &[f32]) -> LearnedPath {
    let mut p = LearnedPath::neutral(m);
    for i in 0..m {
        let b = (i / BIN_SAMPLES).min(scale.len() - 1);
        p.v_scale[i] = scale[b];
        p.aim_m[i] = aim[b];
        p.v_ref[i] = v_ref[b];
    }
    p
}

/// Per-bin values back from a learned path (bin = its first sample).
fn bins_from_path(p: &LearnedPath, bins: usize) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let at = |v: &[f32], b: usize| v[(b * BIN_SAMPLES).min(v.len() - 1)];
    (
        (0..bins).map(|b| at(&p.v_scale, b)).collect(),
        (0..bins).map(|b| at(&p.aim_m, b)).collect(),
        (0..bins).map(|b| at(&p.v_ref, b)).collect(),
    )
}

fn smooth(x: &[f32], closed: bool) -> Vec<f32> {
    let n = x.len();
    (0..n)
        .map(|i| {
            let get = |j: isize| -> f32 {
                if closed {
                    x[j.rem_euclid(n as isize) as usize]
                } else {
                    x[j.clamp(0, n as isize - 1) as usize]
                }
            };
            let i = i as isize;
            0.25 * get(i - 1) + 0.5 * get(i) + 0.25 * get(i + 1)
        })
        .collect()
}

/// Result of training one car on one course.
struct Trained {
    car: String,
    course: String,
    capability: [f32; 4],
    lookahead: f32,
    base: SoloRun,
    best: SoloRun,
    /// The course geometry, one point per bin.
    geom: Vec<[f32; 5]>,
    /// This car's corrections, one per bin.
    entry: CarCourse,
    rounds_kept: u32,
}

/// (incidents, lap time): wall hits, 5 per reset, 1000 for not finishing.
fn score(r: &SoloRun) -> (u32, f32) {
    (
        r.walls + 5 * r.resets + if r.time_s.is_none() { 1000 } else { 0 },
        r.time_s.unwrap_or(f32::INFINITY),
    )
}

/// Steering look-ahead multipliers tried per car (1 = the personality's own).
const LOOKAHEADS: [f32; 3] = [1.2, 1.45, 1.75];

/// `a` beats `b` on incidents first, then lap time.
fn better_pace(a: &SoloRun, b: &SoloRun) -> bool {
    let (ia, ta) = score(a);
    let (ib, tb) = score(b);
    ia < ib || (ia == ib && ta < tb - 0.3)
}

/// `a` beats `b`: no more incidents, and faster, or tracking its line clearly better at
/// about the same pace (lap times scatter by a second from small changes; the line is what
/// the aim correction learns, the pace follows it).
fn better(a: &SoloRun, b: &SoloRun) -> bool {
    let (ia, ta) = score(a);
    let (ib, tb) = score(b);
    let (da, db) = (drift(a), drift(b));
    ia <= ib && (ta < tb - 0.05 || (da.1 * 10 < db.1 * 9 && ta < tb + 0.5))
}

fn train_one(id: &str, car_id: &str, rounds: u32, speed_rounds: u32) -> Trained {
    let base = solo_run(id, car_id, None, 1.0);
    let cap = base.cap;
    // Steering look-ahead first: a car whose yaw answers late weaves at highway speed until
    // it spins, and no line correction cures that.
    let mut la = 1.0f32;
    let mut la_best = base.clone();
    for cand in LOOKAHEADS {
        let run = solo_run(id, car_id, None, cand);
        eprintln!(
            "  {id} {car_id} look-ahead {cand}: {:?} walls {} resets {}",
            run.time_s, run.walls, run.resets
        );
        if better_pace(&run, &la_best) {
            la = cand;
            la_best = run;
        }
    }
    let corridor = base.corridor.clone();
    let m = corridor.len();
    let bins = base.err.len();
    let closed = corridor.closed;
    let (mut scale, mut aim, mut v_ref) =
        (vec![1.0f32; bins], vec![0.0f32; bins], vec![0.0f32; bins]);
    let mut best = la_best.clone();
    let (mut best_scale, mut best_aim, mut best_vref) = (scale.clone(), aim.clone(), v_ref.clone());
    let mut step = 1.0f32;
    let mut kept = 0;
    let mut last = la_best.clone();
    let mut round = 0;
    while round < rounds {
        // Update from the last run.
        let mut new_aim = aim.clone();
        let mut new_vref = v_ref.clone();
        for b in 0..bins {
            let (sum, n, sv) = last.err[b];
            if n == 0 {
                continue;
            }
            let e = sum / n as f32;
            let v = sv / n as f32;
            // The drift seen here was steered for about 0.8 s earlier (lookahead + response).
            let lag = ((v * num("WP_TRAIN_LAG", 0.8f32)) / TRAIN_BIN_M).round() as isize;
            let j = b as isize - lag;
            let j = if closed {
                j.rem_euclid(bins as isize) as usize
            } else if j < 0 {
                continue;
            } else {
                j as usize
            };
            new_aim[j] = (new_aim[j] - num("WP_TRAIN_GAIN", 0.5f32) * step * e).clamp(-1.5, 1.5);
            if new_vref[j] == 0.0 {
                new_vref[j] = v;
            } else {
                new_vref[j] = 0.5 * (new_vref[j] + v);
            }
        }
        let mut new_scale = scale.clone();
        // Line first (the pace follows it), then corner speeds on top of the learned line.
        let speed_phase = round >= rounds.saturating_sub(speed_rounds);
        if speed_phase && round == rounds.saturating_sub(speed_rounds) && speed_rounds > 0 {
            step = 1.0;
        }
        let (learn_aim, learn_speed) = (
            std::env::var("WP_TRAIN_NO_AIM").is_err(),
            speed_phase && std::env::var("WP_TRAIN_NO_SPEED").is_err(),
        );
        if !learn_aim {
            new_aim = aim.clone();
        }
        for b in 0..bins {
            let (sum, n, _) = last.err[b];
            let e = if n > 0 { (sum / n as f32).abs() } else { 0.0 };
            if last.bad[b] || e > 1.2 {
                // Slow the run-in to this place (the braking zone before it).
                for k in 0..6isize {
                    let j = b as isize - k;
                    let j = if closed {
                        j.rem_euclid(bins as isize) as usize
                    } else if j < 0 {
                        break;
                    } else {
                        j as usize
                    };
                    new_scale[j] = (new_scale[j] * (1.0 - 0.03 * step)).max(0.85);
                }
            } else if n > 0 && e < 0.6 {
                new_scale[b] = (new_scale[b] * (1.0 + 0.015 * step)).min(1.12);
            }
        }
        if !learn_speed {
            new_scale = scale.clone();
        }
        let cand_aim = smooth(&new_aim, closed);
        let cand_scale = smooth(&new_scale, closed);
        let path = path_from_bins(m, &cand_scale, &cand_aim, &new_vref);
        let run = solo_run(id, car_id, Some(&path), la);
        eprintln!(
            "  {id} {car_id} round {round}: {:?} walls {} resets {} drift {:.2} m, {} bins > 1 m (best {:?}) step {step}",
            run.time_s,
            run.walls,
            run.resets,
            drift(&run).0,
            drift(&run).1,
            best.time_s
        );
        if better(&run, &best) {
            best = run.clone();
            best_scale = cand_scale.clone();
            best_aim = cand_aim.clone();
            best_vref = new_vref.clone();
            kept += 1;
            scale = cand_scale;
            aim = cand_aim;
            v_ref = new_vref;
            last = run;
        } else {
            // Roll back and take smaller steps from the best so far.
            scale = best_scale.clone();
            aim = best_aim.clone();
            v_ref = best_vref.clone();
            last = best.clone();
            step *= 0.5;
            if step < 0.2 {
                if speed_phase || speed_rounds == 0 {
                    break;
                }
                // Skip ahead to the speed phase.
                round = rounds - speed_rounds;
                continue;
            }
        }
        round += 1;
    }
    let _ = bins_from_path;
    // Stored every POINT_STEP_M: the mean of each pair of bins, at the pair's middle.
    let mut geom = Vec::new();
    let (mut aims, mut vrefs, mut scales) = (Vec::new(), Vec::new(), Vec::new());
    let r1 = |x: f32| (x * 10.0).round() / 10.0;
    let r3 = |x: f32| (x * 1000.0).round() / 1000.0;
    let mean = |v: &[f32], b: usize| {
        let e = (b + BINS_PER_POINT).min(v.len());
        v[b..e].iter().sum::<f32>() / (e - b) as f32
    };
    for b in (0..bins).step_by(BINS_PER_POINT) {
        let i = (b * BIN_SAMPLES + BINS_PER_POINT * BIN_SAMPLES / 2).min(m - 1);
        let smp = &corridor.samples[i];
        geom.push([
            r1(smp.pos[0]),
            r1(smp.pos[1]),
            r1(smp.pos[2]),
            r3(smp.tangent[0]),
            r3(smp.tangent[1]),
        ]);
        aims.push((mean(&best_aim, b) * 10.0).round().clamp(-15.0, 15.0) as i8 as u8);
        vrefs.push(mean(&best_vref, b).round().clamp(0.0, 255.0) as u8);
        scales.push(
            ((mean(&best_scale, b) - 1.0) * 200.0)
                .round()
                .clamp(-30.0, 24.0) as i8 as u8,
        );
    }
    let entry = CarCourse {
        course: id.to_string(),
        aim_dm: to_hex(aims),
        v_ref: to_hex(vrefs),
        v_scale: if scales.iter().any(|x| *x != 0) {
            to_hex(scales)
        } else {
            String::new()
        },
        lookahead_scale: la,
    };
    Trained {
        car: car_id.to_string(),
        course: id.to_string(),
        capability: [
            cap.max_lat_g,
            cap.brake_g,
            cap.accel_mps2,
            cap.top_speed_mps,
        ],
        lookahead: la,
        base,
        best,
        geom,
        entry,
        rounds_kept: kept,
    }
}

fn load_table() -> LearnedTable {
    std::fs::read_to_string(table_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_table(t: &LearnedTable) {
    let mut s = serde_json::to_string(t).unwrap();
    // One course / car entry per line keeps diffs readable.
    s = s.replace("},{", "},\n{");
    std::fs::write(table_path(), s + "\n").unwrap();
}

fn run_parallel<T: Send, R: Send>(jobs: Vec<T>, f: impl Fn(T) -> R + Sync) -> Vec<R> {
    let threads = num("WP_TRAIN_THREADS", 4usize).max(1);
    let jobs: Vec<(usize, T)> = jobs.into_iter().enumerate().collect();
    let queue = std::sync::Mutex::new(jobs);
    let out = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|sc| {
        for _ in 0..threads {
            sc.spawn(|| loop {
                let job = queue.lock().unwrap().pop();
                let Some((i, j)) = job else { break };
                let r = f(j);
                out.lock().unwrap().push((i, r));
            });
        }
    });
    let mut out = out.into_inner().unwrap();
    out.sort_by_key(|(i, _)| *i);
    out.into_iter().map(|(_, r)| r).collect()
}

#[test]
#[ignore = "training run (minutes per car in release): writes data/ai/racer_training.json with WP_TRAIN_WRITE=1"]
fn stage1() {
    let courses: Vec<String> = list("WP_TRAIN_RACES", &COURSES)
        .into_iter()
        .filter(|id| catalog::race(id).is_some() && scenario_path(id).exists())
        .collect();
    let cars = std::env::var("WP_TRAIN_CARS")
        .map(|v| v.split(',').map(|x| x.trim().to_string()).collect())
        .unwrap_or_else(|_| default_cars());
    let rounds = num("WP_TRAIN_ROUNDS", 12u32);
    let speed_rounds = num("WP_TRAIN_SPEED_ROUNDS", 4u32);
    let jobs: Vec<(String, String)> = courses
        .iter()
        .flat_map(|c| cars.iter().map(move |k| (c.clone(), k.clone())))
        .collect();
    let results = run_parallel(jobs, |(c, k)| train_one(&c, &k, rounds, speed_rounds));
    let mut table = load_table();
    for t in &results {
        eprintln!(
            "{:<22} {:<16} untrained {:>7.1?} s walls {:>3} resets {} -> trained {:>7.1?} s walls {:>3} resets {} (look-ahead x{}, {} rounds kept, drift {:.2} -> {:.2} m)",
            t.course,
            t.car,
            t.base.time_s,
            t.base.walls,
            t.base.resets,
            t.best.time_s,
            t.best.walls,
            t.best.resets,
            t.lookahead,
            t.rounds_kept,
            drift(&t.base).0,
            drift(&t.best).0,
        );
        table.courses.retain(|g| g.course != t.course);
        table.courses.push(CourseGeom {
            course: t.course.clone(),
            points: t.geom.clone(),
        });
        let entry = match table.cars.iter_mut().position(|c| c.car == t.car) {
            Some(i) => &mut table.cars[i],
            None => {
                table.cars.push(LearnedCar {
                    car: t.car.clone(),
                    capability: t.capability,
                    courses: Vec::new(),
                });
                table.cars.last_mut().unwrap()
            }
        };
        entry.capability = t.capability;
        entry.courses.retain(|c| c.course != t.course);
        entry.courses.push(t.entry.clone());
        entry.courses.sort_by(|a, b| a.course.cmp(&b.course));
    }
    table.courses.sort_by(|a, b| a.course.cmp(&b.course));
    table.cars.sort_by(|a, b| a.car.cmp(&b.car));
    if std::env::var("WP_TRAIN_WRITE").is_ok() {
        write_table(&table);
        eprintln!("wrote {}", table_path().display());
    }
    for t in &results {
        assert!(
            score(&t.best) <= score(&t.base),
            "{} {}: training made it worse",
            t.course,
            t.car
        );
    }
}

#[test]
#[ignore = "release check of the embedded table (minutes): every trained car untrained vs as the game drives it; WP_TRAIN_WRITE=1 drops the car courses that add incidents"]
fn stage1_check() {
    let table = LearnedTable::embedded();
    let courses: Vec<String> = list("WP_TRAIN_RACES", &COURSES)
        .into_iter()
        .filter(|id| catalog::race(id).is_some() && scenario_path(id).exists())
        .collect();
    let cars: Vec<String> = std::env::var("WP_TRAIN_CARS")
        .map(|v| v.split(',').map(|x| x.trim().to_string()).collect())
        .unwrap_or_else(|_| table.cars.iter().map(|c| c.car.clone()).collect());
    let jobs: Vec<(String, String)> = courses
        .iter()
        .flat_map(|c| cars.iter().map(move |k| (c.clone(), k.clone())))
        .collect();
    let runs = run_parallel(jobs.clone(), |(c, k)| {
        (
            solo_run(&c, &k, None, 1.0),
            solo_run(&c, &k, None, f32::NAN),
        )
    });
    let mut worse = Vec::new();
    for ((c, k), (raw, game)) in jobs.iter().zip(&runs) {
        eprintln!(
            "{c:<22} {k:<16} untrained {:>7.1?} s walls {:>3} resets {} | embedded {:>7.1?} s walls {:>3} resets {}",
            raw.time_s, raw.walls, raw.resets, game.time_s, game.walls, game.resets
        );
        if score(game).0 > score(raw).0 + 2 {
            worse.push((c.clone(), k.clone()));
        }
    }
    if std::env::var("WP_TRAIN_WRITE").is_ok() && !worse.is_empty() {
        // Validation failed: that car drives untrained on that course. (Chaotic courses: a
        // correction that helped in training can tip a car on the edge into a wall under
        // another seed or start, and the shipped table must never add crashes.)
        let mut t = load_table();
        for (c, k) in &worse {
            let course = trained_course_of(c);
            if let Some(car) = t.cars.iter_mut().find(|x| &x.car == k) {
                car.courses.retain(|x| x.course != course);
            }
        }
        t.cars.retain(|c| !c.courses.is_empty());
        write_table(&t);
        eprintln!("pruned {} car courses: {worse:?}", worse.len());
        return;
    }
    assert!(
        worse.is_empty(),
        "more incidents with the embedded table: {worse:?}"
    );
}

/// The trained course whose rows race `id` drives on (Loop 9 day runs the night race's road).
fn trained_course_of(id: &str) -> &str {
    match id {
        "loop9_day" => "loop9_circuit",
        other => other,
    }
}

/// One full race with traffic and police: what stage 2 scores.
#[derive(Debug, Clone, Copy, Default)]
struct RaceOutcome {
    cars: u32,
    finished: u32,
    captured: u32,
    /// Judge-counted civilian hits, all cars.
    civ_hits: u32,
    /// Mean finishing time of the finishers, s.
    mean_time_s: f32,
    rams: u32,
    dodges: u32,
    /// Cars with a punctured tyre at the end.
    punctured: u32,
}

impl RaceOutcome {
    /// Higher is better: finishers first, then clean driving (civilians are a DQ in the
    /// game's eyes and a crash in the player's), then pace.
    fn score(&self) -> f32 {
        10.0 * self.finished as f32
            - 3.0 * self.civ_hits as f32
            - 4.0 * self.captured as f32
            - 2.0 * self.punctured as f32
            - self.mean_time_s / 30.0
    }
}

/// Race `id` with its whole field (catalog personalities, the player's seat AI-driven too),
/// the scenario's own traffic and police, seed `seed`, trained numbers `traffic`.
fn full_race(id: &str, seed: u64, traffic: LearnedTraffic) -> RaceOutcome {
    let mut sc = load_scenario(&scenario_path(id)).unwrap();
    sc.seed = seed;
    let data_src = FsData::new(repo().join("data"));
    let built = build_sim(&sc, &data_src, &RunOptions::default()).unwrap();
    let mut sim = built.sim;
    for &e in &built.ids {
        sim.set_player_steering(e, Some(false)).unwrap();
    }
    let def = catalog::race(id).unwrap();
    let race = catalog::plan_course(sim.world(), &def).unwrap();
    let mut judge = RaceJudge::new(EventId(1), def.rules(), race.course.clone());
    // The AI field (pole first), then the player's seat (entity 0).
    let n = sc.entities.len();
    let order: Vec<usize> = (1..n).chain(std::iter::once(0)).collect();
    let mut entrants = Vec::new();
    for &i in &order {
        let spec = &sc.entities[i];
        let car: &LoadedCar = &built.cars[&spec.car];
        judge
            .enter(RacerEntry {
                racer: built.ids[i],
                redline_rpm: car.def.engine.redline_rpm,
            })
            .unwrap();
        if i > 0 {
            sim.set_vehicle_kind(built.ids[i], VehicleKind::AiRacer)
                .unwrap();
        }
        let profile = if i > 0 {
            def.ai.get(i - 1).map(|a| a.profile).unwrap_or_default()
        } else {
            DriverProfile::default()
        };
        entrants.push(AiEntrant {
            racer: built.ids[i],
            profile,
            capability: CarCapability::for_car(car),
        });
    }
    let closed = def.legality == Legality::Sanctioned;
    let mut ai = AiEntrants::new(sim.world(), &race, def.kind, closed, &entrants, seed)
        .expect("a drivable route");
    ai.field_mut().set_traffic(traffic);
    let racers: Vec<EntityId> = entrants.iter().map(|e| e.racer).collect();
    judge.set_stranded_reset(&racers);
    let green = judge.start(sim.tick()).unwrap();
    // The race's own police, as the game sets it up: doctrine plus the scripted roadblocks,
    // spike strips and stakeouts (without this stage 2 never meets a spike strip).
    wp_race::police::start_race(sim.police_mut(), &def, &race, &racers);
    let mut events: Vec<SimEvent> = Vec::new();
    let max_s = (race.route.length_m / 8.0 + 90.0) as u64;
    while sim.tick() < green + 240 * max_s {
        let tick = sim.tick();
        let inputs = ai.tick(
            &judge,
            tick,
            sim.world(),
            &obs(&sim),
            &events,
            &SimPolice(sim.police()),
        );
        for (e, inp) in inputs {
            sim.set_input(e, inp).unwrap();
        }
        sim.step_fixed();
        events = sim.drain_events();
        let samples: Vec<RacerSample> = sim
            .vehicles()
            .iter()
            .filter(|v| racers.contains(&v.id))
            .map(|v| RacerSample::from_state(v.id, &v.vehicle.state(), v.input.throttle))
            .collect();
        judge.observe(sim.tick(), &samples, &events);
        ai.report_to(&mut judge, sim.tick());
        for r in judge.drain_resets() {
            let yaw = wp_math::atan2(-r.dir[0], r.dir[1]);
            sim.place_on_ground(r.racer, [r.pos[0], r.pos[1]], yaw, 0.0, Some(r.pos[2]))
                .unwrap();
            sim.set_ghost(r.racer, 240).unwrap();
            judge.relocate(r.racer, r.pos, sim.tick());
            ai.relocate(r.racer, r.pos);
        }
        if judge
            .results()
            .iter()
            .all(|r| r.status != wp_race::RacerStatus::Running)
        {
            break;
        }
    }
    let mut out = RaceOutcome {
        cars: racers.len() as u32,
        ..Default::default()
    };
    let mut times = Vec::new();
    for r in judge.results() {
        out.civ_hits += r.civilian_hits;
        match r.status {
            wp_race::RacerStatus::Finished => {
                out.finished += 1;
                if let Some(t) = r.elapsed_subticks {
                    times.push(t as f32 * PHYSICS_DT / wp_race::result::SUBTICKS_PER_TICK as f32);
                }
            }
            wp_race::RacerStatus::Dnf(wp_race::DnfReason::Captured) => out.captured += 1,
            _ => {}
        }
        if sim.police().tire_puncture(r.racer).iter().any(|p| *p > 0.0) {
            out.punctured += 1;
        }
    }
    out.mean_time_s = if times.is_empty() {
        race.route.length_m / 8.0
    } else {
        times.iter().sum::<f32>() / times.len() as f32
    };
    for k in 0..racers.len() {
        out.rams += ai.field().rams_by(k);
        out.dodges += ai.field().spike_dodges_by(k);
    }
    out
}

/// Sum of outcomes over seeds.
fn total(outs: &[RaceOutcome]) -> (f32, RaceOutcome) {
    let mut t = RaceOutcome::default();
    let mut score = 0.0;
    for o in outs {
        score += o.score();
        t.cars += o.cars;
        t.finished += o.finished;
        t.captured += o.captured;
        t.civ_hits += o.civ_hits;
        t.mean_time_s += o.mean_time_s / outs.len() as f32;
        t.rams += o.rams;
        t.dodges += o.dodges;
        t.punctured += o.punctured;
    }
    (score / outs.len().max(1) as f32, t)
}

/// Stage 2 races: the kept Loop 9 races, plus two retired picker races kept as training
/// grounds for the police tools Loop 9 at night does not use: the Grid highway sprint (a
/// scripted spike strip across half the road, rolling blocks and PITs) and the Lantern Row
/// checkpoint run (roadblocks across the route, every unit in town). Ids not in the catalog
/// are skipped.
const STAGE2_RACES: [&str; 5] = [
    "loop9_circuit",
    "loop9_day",
    "loop9_ashpeak_highway",
    "grid_highway_sprint",
    "lantern_checkpoint",
];

/// The values stage 2 tries for each trained number (coordinate search, one at a time).
fn stage2_axes() -> Vec<(&'static str, Vec<f32>)> {
    vec![
        ("civilian_margin_scale", vec![0.85, 1.0, 1.15]),
        ("civilian_headway_scale", vec![0.85, 1.0, 1.15]),
        ("ram_min_nerve", vec![0.8, 1.0, 1.2, 2.1]),
        ("ram_closing_mps", vec![4.0, 6.0, 9.0]),
        ("spike_clearance_m", vec![0.4, 0.6, 0.9]),
    ]
}

fn with(t: LearnedTraffic, axis: &str, x: f32) -> LearnedTraffic {
    let mut t = t;
    match axis {
        "civilian_margin_scale" => t.civilian_margin_scale = x,
        "civilian_headway_scale" => t.civilian_headway_scale = x,
        "ram_min_nerve" => t.ram_min_nerve = x,
        "ram_closing_mps" => t.ram_closing_mps = x,
        "spike_clearance_m" => t.spike_clearance_m = x,
        _ => unreachable!(),
    }
    t
}

#[test]
#[ignore = "training run (an hour in release): writes the traffic block of data/ai/racer_training.json with WP_TRAIN_WRITE=1"]
fn stage2() {
    let courses: Vec<String> = list("WP_TRAIN_RACES", &STAGE2_RACES)
        .into_iter()
        .filter(|id| catalog::race(id).is_some() && scenario_path(id).exists())
        .collect();
    let seeds: Vec<u64> = list("WP_TRAIN_SEEDS", &["1", "2", "3"])
        .iter()
        .map(|x| x.parse().unwrap())
        .collect();
    let mut table = load_table();
    let mut best = table.traffic;
    let evaluate = |t: LearnedTraffic| -> (f32, RaceOutcome) {
        let jobs: Vec<(String, u64)> = courses
            .iter()
            .flat_map(|c| seeds.iter().map(move |s| (c.clone(), *s)))
            .collect();
        total(&run_parallel(jobs, |(c, s)| full_race(&c, s, t)))
    };
    let (mut best_score, base) = evaluate(best);
    eprintln!("stage2 start {best:?}: score {best_score:.2} {base:?}");
    for (axis, values) in stage2_axes() {
        for x in values {
            let cand = with(best, axis, x);
            if cand == best {
                continue;
            }
            let (score, t) = evaluate(cand);
            eprintln!("  {axis} = {x}: score {score:.2} {t:?}");
            if score > best_score + 0.5 {
                best_score = score;
                best = cand;
            }
        }
        eprintln!("stage2 after {axis}: {best:?} score {best_score:.2}");
    }
    table.traffic = best;
    if std::env::var("WP_TRAIN_WRITE").is_ok() {
        write_table(&table);
        eprintln!("wrote {}", table_path().display());
    }
}
