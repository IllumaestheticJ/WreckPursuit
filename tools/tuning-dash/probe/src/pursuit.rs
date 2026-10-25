//! `pursuit` probe: a simcli city scenario with the police tables read from the (overridden)
//! data root, and every police contact classified.
//!
//! `simcli::run` builds police from `wp_police::PoliceConfig::builtin()` (the tables compiled
//! into the crate), so sweeping `data/police/*.toml` needs its own loop. This one builds the
//! sim with [`simcli::run::build_sim`] (identical setup), swaps in a `PoliceSystem` made from
//! the overlay's tables, then ticks it exactly like `simcli::run` does (same drivers, same
//! order). With no police overrides the final state hash equals `simcli run`'s.
//!
//! A **hit** is a run of `Contact` events between one police car and one other body with no
//! gap longer than [`HIT_GAP_TICKS`]. Each hit records both speeds just before it, the
//! largest closing speed during it, and what the police unit was doing (assignment, tactic).
//! The struck body counts as **stopped** below `stopped_mps`.
//!
//! `police_entities` marks scenario entities as police cars (`Sim::set_vehicle_kind`, no
//! physics change, no AI): a deliberate stand-in such as a cop held on the brakes, for fixtures
//! like `tools/tuning-dash/scenarios/racer_into_stopped_cop.ron`. Their contacts count as police
//! hits with an empty assignment and tactic.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;
use wp_contracts::{EntityId, SimEvent, VehicleInput, VehicleSim, PHYSICS_DT};
use wp_police::{PoliceConfig, PoliceSetup, PoliceSystem};
use wp_sim::VehicleKind;

use crate::frames::{FrameCapture, Scenery, DEFAULT_FRAMES};
use crate::overrides::Overlay;

/// Contact gap that ends a hit, ticks (0.25 s).
pub const HIT_GAP_TICKS: u64 = 60;

/// Default speed below which a struck car counts as stopped, m/s.
pub const DEFAULT_STOPPED_MPS: f32 = 1.0;

/// Police RNG salt used by simcli (`run::police_for_city`).
const POLICE_SEED_SALT: u64 = 0x0090_11CE;

/// `min_pursuer_gap_m` ignores the first seconds of a run, s.
pub const GAP_SKIP_S: f32 = 3.0;

/// Hits kept in `info.hits` (the counts cover all of them).
const MAX_LISTED_HITS: usize = 40;

/// What a police car hit. Serialized as its [`Struck::label`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Struck {
    /// A scenario entity, by scenario name.
    Scenario(String),
    /// Physical traffic.
    Traffic,
    /// Another police car.
    Police,
    /// Static world, props or anything that is not a vehicle.
    World,
}

impl Struck {
    /// `scenario:<name>`, `traffic`, `police` or `world`.
    pub fn label(&self) -> String {
        match self {
            Struck::Scenario(n) => format!("scenario:{n}"),
            Struck::Traffic => "traffic".into(),
            Struck::Police => "police".into(),
            Struck::World => "world".into(),
        }
    }
}

impl Serialize for Struck {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.label())
    }
}

/// One police hit.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hit {
    /// Start time, s.
    pub t_s: f32,
    /// What was hit.
    pub struck: Struck,
    /// Police car speed just before the hit, m/s.
    pub police_mps: f32,
    /// Struck body's speed just before the hit, m/s (0 for the world).
    pub struck_mps: f32,
    /// Largest closing speed during the hit, m/s.
    pub closing_mps: f32,
    /// Largest normal impulse during the hit, N·s.
    pub impulse_ns: f32,
    /// Police assignment at the start (`Patrol`, `Pursue`, ...).
    pub assignment: String,
    /// Police tactic at the start (`Follow`, `Pit`, ...).
    pub tactic: String,
    /// Struck body was below the stopped threshold.
    pub stopped: bool,
}

/// Police tables from the overlay (`police/*.toml`), or the built-in ones when the data root
/// has none, with `dense_patrols`, `police_tools` and `police_presence` applied like simcli does.
pub fn police_config(data: &Overlay, sc: &simcli::Scenario) -> Result<PoliceConfig, String> {
    let names = ["heat", "presence", "pursuit", "tools"];
    let texts: Vec<Result<String, String>> = names
        .iter()
        .map(|n| data.text(&format!("police/{n}.toml")))
        .collect();
    let mut cfg = if texts.iter().all(|t| t.is_ok()) {
        let t: Vec<String> = texts.into_iter().map(|t| t.unwrap_or_default()).collect();
        PoliceConfig::parse(&t[0], &t[1], &t[2], &t[3])
            .map_err(|e| format!("police tables: {e}"))?
    } else {
        PoliceConfig::builtin()
    };
    if sc.dense_patrols {
        cfg.presence.patrols = wp_police::config::dense_patrols();
    }
    if !sc.police_tools {
        let u = &mut cfg.presence.unlocks;
        u.spikes_level = u8::MAX;
        u.roadblock_level = u8::MAX;
    }
    if let Some(p) = sc.police_presence {
        let scale = |n: &mut u32| *n = (*n as f32 * p).round() as u32;
        let t = &mut cfg.presence.patrols;
        for d in [&mut t.core, &mut t.suburb, &mut t.industrial] {
            for n in [&mut d.dawn, &mut d.day, &mut d.dusk, &mut d.night] {
                scale(n);
            }
        }
    }
    Ok(cfg)
}

/// Result of a pursuit run.
#[derive(Debug, Clone, Default)]
pub struct PursuitRun {
    /// Flat metrics.
    pub metrics: BTreeMap<String, Option<f64>>,
    /// Every hit, in time order.
    pub hits: Vec<Hit>,
    /// Final `Sim::state_hash`, hex (same format as simcli).
    pub final_hash: String,
    /// simcli build warnings.
    pub warnings: Vec<String>,
    /// Frames, when asked for.
    pub frames: Option<(Vec<crate::frames::Frame>, Scenery)>,
    /// Top-down path per scenario entity (thumbnail fallback).
    pub thumb: BTreeMap<String, Vec<[f32; 2]>>,
}

fn variant_name<T: std::fmt::Debug>(v: &T) -> String {
    let s = format!("{v:?}");
    s.split([' ', '{', '(']).next().unwrap_or("").to_string()
}

/// Run `sc` against `data` (police tables included). `frames`: take top-down frames.
/// `police_entities`: scenario entity ids to treat as police cars (see the module docs).
pub fn run(
    sc: &simcli::Scenario,
    data: &Overlay,
    stopped_mps: f32,
    frames: bool,
    police_entities: &[String],
) -> Result<PursuitRun, String> {
    let cfg = police_config(data, sc)?;
    let built = simcli::run::build_sim(sc, data, &simcli::RunOptions::default())
        .map_err(|e| e.to_string())?;
    if !built.has_city {
        return Err("the pursuit probe needs a city scenario (city: procedural(...))".into());
    }
    let simcli::run::BuiltSim {
        mut sim,
        ids,
        cars,
        warnings,
        has_city,
        ..
    } = built;
    if sc.police_cars {
        // Same construction as simcli's `police_for_city`, with the overlay's tables.
        let mut police = PoliceSystem::for_world(
            cfg,
            sim.world(),
            PoliceSetup {
                seed: sc.seed ^ POLICE_SEED_SALT,
                mode: sc.mode,
                internal_driving: true,
                hazards: sc.traffic_cars,
            },
        );
        for (spec, id) in sc.entities.iter().zip(&ids) {
            if spec.police_heat > 0.0 {
                police.set_heat(*id, spec.police_heat);
            }
        }
        sim.set_police(police);
    }
    for name in police_entities {
        let i = sc
            .entities
            .iter()
            .position(|e| &e.id == name)
            .ok_or_else(|| format!("police_entities: no scenario entity {name:?}"))?;
        sim.set_vehicle_kind(ids[i], VehicleKind::Police)
            .map_err(|e| e.to_string())?;
    }

    let n = ids.len();
    let names: Vec<String> = sc.entities.iter().map(|e| e.id.clone()).collect();
    let rest: Vec<Vec<[f32; 3]>> = sc
        .entities
        .iter()
        .map(|e| {
            cars[&e.car]
                .lattice
                .nodes
                .iter()
                .map(|nd| nd.pos_m)
                .collect()
        })
        .collect();
    let mut crush = vec![0.0f32; n];
    let mut total_loss = vec![false; n];
    let mut path_len = vec![0.0f32; n];
    let mut last_pos: Vec<[f32; 3]> = sc.entities.iter().map(|e| e.pos).collect();
    let mut thumb: BTreeMap<String, Vec<[f32; 2]>> = BTreeMap::new();

    let mut sizes = BTreeMap::new();
    let mut capture = if frames {
        for v in sim.vehicles() {
            if let Some(c) = cars.get(&v.car_id) {
                sizes.insert(
                    v.car_id.clone(),
                    [c.def.body_size_m[0], c.def.body_size_m[1]],
                );
            }
        }
        for id in [simcli::run::POLICE_CAR, simcli::run::TRAFFIC_CAR] {
            if let Ok(c) = simcli::data::load_car(data, id) {
                sizes.insert(id.to_string(), [c.def.body_size_m[0], c.def.body_size_m[1]]);
            }
        }
        Some(FrameCapture::new(sc.ticks(), DEFAULT_FRAMES, sizes))
    } else {
        None
    };

    let mut last_inputs = vec![VehicleInput::default(); n];
    let mut prev_speed: BTreeMap<u32, f32> = BTreeMap::new();
    let mut open: BTreeMap<(u32, u32), (usize, u64)> = BTreeMap::new();
    let mut hits: Vec<Hit> = Vec::new();
    let mut first_kind: BTreeMap<u16, u64> = BTreeMap::new();
    let mut kind_counts: BTreeMap<u16, u64> = BTreeMap::new();
    let mut units_max = 0usize;
    let mut cars_max = 0usize;
    // Closest physical pursuer (Pursue / Intercept) to any scenario car after the first
    // GAP_SKIP_S (spawn positions say nothing about the chase), and time spent pursuing.
    let mut min_gap = f32::INFINITY;
    let mut pursue_ticks = 0u64;
    let ticks = sc.ticks();
    let sample_every = sc.sample_every();

    for tick in 0..ticks {
        let states = simcli::run::scenario_states(&sim, &ids);
        for (idx, id) in ids.iter().enumerate() {
            let input = simcli::run::driver_input(sc, idx, tick, &states[idx].1, &last_inputs[idx]);
            if input != last_inputs[idx] {
                sim.set_input(*id, input).map_err(|e| e.to_string())?;
                last_inputs[idx] = input;
            }
        }
        sim.step_fixed();
        let events = sim.drain_events();
        let now = sim.tick();

        // Who is who, this tick.
        let kind_of: BTreeMap<u32, (VehicleKind, Option<wp_police::UnitId>)> = sim
            .vehicles()
            .iter()
            .map(|v| (v.id.0, (v.kind, v.police_unit)))
            .collect();
        let scenario_index = |e: EntityId| ids.iter().position(|x| *x == e);
        for e in &events {
            match e {
                SimEvent::Contact {
                    a,
                    b,
                    rel_speed,
                    impulse_ns,
                    ..
                } => {
                    let is_police = |x: &EntityId| {
                        kind_of
                            .get(&x.0)
                            .is_some_and(|(k, _)| *k == VehicleKind::Police)
                    };
                    let (cop, other) = if is_police(a) {
                        (*a, *b)
                    } else if is_police(b) {
                        (*b, *a)
                    } else {
                        continue;
                    };
                    let key = (cop.0, other.0);
                    let closing = rel_speed.abs();
                    if let Some((i, last)) = open.get_mut(&key) {
                        if now - *last <= HIT_GAP_TICKS {
                            *last = now;
                            let h = &mut hits[*i];
                            h.closing_mps = h.closing_mps.max(closing);
                            h.impulse_ns = h.impulse_ns.max(*impulse_ns);
                            continue;
                        }
                    }
                    let struck = if let Some(i) = scenario_index(other) {
                        Struck::Scenario(names[i].clone())
                    } else {
                        match kind_of.get(&other.0).map(|(k, _)| *k) {
                            Some(VehicleKind::Police) => Struck::Police,
                            Some(VehicleKind::Traffic) => Struck::Traffic,
                            Some(_) => Struck::Scenario("other".into()),
                            None => Struck::World,
                        }
                    };
                    let struck_mps = if struck == Struck::World {
                        0.0
                    } else {
                        prev_speed.get(&other.0).copied().unwrap_or(0.0)
                    };
                    let unit = kind_of
                        .get(&cop.0)
                        .and_then(|(_, u)| *u)
                        .and_then(|u| sim.police().units().iter().find(|x| x.id == u));
                    hits.push(Hit {
                        t_s: now as f32 * PHYSICS_DT,
                        stopped: struck != Struck::World && struck_mps < stopped_mps,
                        struck,
                        police_mps: prev_speed.get(&cop.0).copied().unwrap_or(0.0),
                        struck_mps,
                        closing_mps: closing,
                        impulse_ns: *impulse_ns,
                        assignment: unit
                            .map(|u| variant_name(&u.assignment))
                            .unwrap_or_default(),
                        tactic: unit.map(|u| variant_name(&u.tactic)).unwrap_or_default(),
                    });
                    open.insert(key, (hits.len() - 1, now));
                }
                SimEvent::Police(pe) => {
                    let kind = pe.kind();
                    first_kind.entry(kind).or_insert(now);
                    *kind_counts.entry(kind).or_default() += 1;
                }
                SimEvent::TotalLoss { vehicle } => {
                    if let Some(i) = scenario_index(*vehicle) {
                        total_loss[i] = true;
                    }
                }
                _ => {}
            }
        }

        // Speeds for the next tick's hits; scenario-car trackers.
        prev_speed = sim
            .vehicles()
            .iter()
            .map(|v| (v.id.0, v.vehicle.state().speed_mps))
            .collect();
        for (i, id) in ids.iter().enumerate() {
            let s = sim
                .vehicles()
                .iter()
                .find(|v| v.id == *id)
                .map(|v| v.vehicle.state());
            if let Some(s) = s {
                let d = ((s.pos[0] - last_pos[i][0]).powi(2) + (s.pos[1] - last_pos[i][1]).powi(2))
                    .sqrt();
                path_len[i] += d;
                last_pos[i] = s.pos;
                if now.is_multiple_of(sample_every) || now == ticks {
                    if let Ok(nodes) = sim.node_positions(*id) {
                        crush[i] = crush[i].max(simcli::run::crush_depth(&rest[i], nodes));
                    }
                    if thumb.get(&names[i]).is_none_or(|t| t.len() < 400)
                        && now.is_multiple_of(sample_every * 4)
                    {
                        thumb
                            .entry(names[i].clone())
                            .or_default()
                            .push([s.pos[0], s.pos[1]]);
                    }
                }
            }
        }
        units_max = units_max.max(sim.police().units().len());
        let mut pursuing = false;
        for v in sim
            .vehicles()
            .iter()
            .filter(|v| v.kind == VehicleKind::Police)
        {
            let Some(u) = v
                .police_unit
                .and_then(|u| sim.police().units().iter().find(|x| x.id == u))
            else {
                continue;
            };
            if !matches!(
                u.assignment,
                wp_police::Assignment::Pursue { .. } | wp_police::Assignment::Intercept { .. }
            ) {
                continue;
            }
            pursuing = true;
            if (now as f32) * PHYSICS_DT < GAP_SKIP_S {
                continue;
            }
            let p = v.vehicle.state().pos;
            for (_, s) in simcli::run::scenario_states(&sim, &ids) {
                let d = ((p[0] - s.pos[0]).powi(2) + (p[1] - s.pos[1]).powi(2)).sqrt();
                min_gap = min_gap.min(d);
            }
        }
        if pursuing {
            pursue_ticks += 1;
        }
        cars_max = cars_max.max(
            sim.vehicles()
                .iter()
                .filter(|v| v.kind == VehicleKind::Police)
                .count(),
        );
        if let Some(c) = capture.as_mut() {
            c.on_tick(&sim, &events, n);
        }
    }

    let mut m: BTreeMap<String, Option<f64>> = BTreeMap::new();
    let mut put = |k: &str, v: Option<f64>| {
        m.insert(k.to_string(), v);
    };
    let scen = |h: &&Hit| matches!(h.struck, Struck::Scenario(_));
    let count = |f: &dyn Fn(&&Hit) -> bool| Some(hits.iter().filter(|h| f(h)).count() as f64);
    let max_closing = |f: &dyn Fn(&&Hit) -> bool| {
        Some(
            hits.iter()
                .filter(|h| f(h))
                .map(|h| h.closing_mps as f64)
                .fold(0.0, f64::max),
        )
    };
    put("police_hits", Some(hits.len() as f64));
    put("hits_stopped_scenario", count(&|h| scen(h) && h.stopped));
    put("hits_moving_scenario", count(&|h| scen(h) && !h.stopped));
    put(
        "hits_stopped_traffic",
        count(&|h| h.struck == Struck::Traffic && h.stopped),
    );
    put(
        "hits_moving_traffic",
        count(&|h| h.struck == Struck::Traffic && !h.stopped),
    );
    put("hits_stopped_any", count(&|h| h.stopped));
    put("hits_police_police", count(&|h| h.struck == Struck::Police));
    put("hits_world", count(&|h| h.struck == Struck::World));
    put(
        "hits_patrol_on_stopped",
        count(&|h| h.stopped && h.assignment != "Pursue" && h.assignment != "Intercept"),
    );
    put("max_closing_stopped_mps", max_closing(&|h| h.stopped));
    put("max_closing_scenario_mps", max_closing(&|h| scen(h)));
    put(
        "first_stopped_hit_s",
        hits.iter().find(|h| h.stopped).map(|h| h.t_s as f64),
    );
    let tick_s = |k: u16| first_kind.get(&k).map(|t| *t as f64 * PHYSICS_DT as f64);
    use wp_police::events::kind as pk;
    put("pursuit_started_s", tick_s(pk::PURSUIT_STARTED));
    put(
        "pursuits_started",
        Some(*kind_counts.get(&pk::PURSUIT_STARTED).unwrap_or(&0) as f64),
    );
    put(
        "pit_attempts",
        Some(*kind_counts.get(&pk::PIT_ATTEMPT).unwrap_or(&0) as f64),
    );
    put(
        "units_disabled",
        Some(*kind_counts.get(&pk::UNIT_DISABLED).unwrap_or(&0) as f64),
    );
    put("police_units_max", Some(units_max as f64));
    put("police_cars_max", Some(cars_max as f64));
    put(
        "min_pursuer_gap_m",
        min_gap.is_finite().then_some(min_gap as f64),
    );
    put(
        "pursue_time_s",
        Some(pursue_ticks as f64 * PHYSICS_DT as f64),
    );
    let finals = simcli::run::scenario_states(&sim, &ids);
    for (i, (_, s)) in finals.iter().enumerate() {
        let id = &names[i];
        put(&format!("{id}.final_health"), Some(s.health as f64));
        put(&format!("{id}.final_speed_mps"), Some(s.speed_mps as f64));
        put(&format!("{id}.crush_depth_m"), Some(crush[i] as f64));
        put(
            &format!("{id}.total_loss"),
            Some(if total_loss[i] { 1.0 } else { 0.0 }),
        );
        put(&format!("{id}.path_length_m"), Some(path_len[i] as f64));
    }
    let frames = capture.map(|c| (c.frames, Scenery::from_sim(&sim, has_city, &sc.barriers)));
    Ok(PursuitRun {
        metrics: m,
        hits,
        final_hash: format!("{:016x}", sim.state_hash()),
        warnings,
        frames,
        thumb,
    })
}

/// `info` entries for a run: the first hits and the hash.
pub fn info(run: &PursuitRun) -> BTreeMap<String, Value> {
    let mut info = BTreeMap::new();
    let listed: Vec<&Hit> = run.hits.iter().take(MAX_LISTED_HITS).collect();
    info.insert(
        "hits".into(),
        serde_json::to_value(listed).unwrap_or(Value::Null),
    );
    info.insert("final_hash".into(), Value::from(run.final_hash.clone()));
    info.insert("warnings".into(), Value::from(run.warnings.clone()));
    info
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn repo() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
    }

    /// golden/city_chase cut to 0.5 s.
    fn short_chase() -> simcli::Scenario {
        let mut sc =
            simcli::load_scenario(&repo().join("apps/simcli/scenarios/golden/city_chase.ron"))
                .unwrap();
        sc.duration_s = 0.5;
        sc
    }

    #[test]
    fn same_final_hash_as_simcli_run() {
        let sc = short_chase();
        let data = Overlay::new(repo().join("data"));
        let ours = run(&sc, &data, DEFAULT_STOPPED_MPS, false, &[]).unwrap();
        let theirs = simcli::run(&sc, &data, &simcli::RunOptions::default()).unwrap();
        assert_eq!(ours.final_hash, theirs.summary.final_hash);
    }

    /// Tuning-dash fixture: the racer rear-ends a stand-in police car held on the brakes.
    fn racer_into_stopped_cop() -> simcli::Scenario {
        simcli::load_scenario(
            &repo().join("tools/tuning-dash/scenarios/racer_into_stopped_cop.ron"),
        )
        .unwrap()
    }

    #[test]
    fn racer_contact_classified() {
        let sc = racer_into_stopped_cop();
        let data = Overlay::new(repo().join("data"));
        let ours = run(&sc, &data, DEFAULT_STOPPED_MPS, false, &["cop".into()]).unwrap();
        let racer_hits: Vec<&Hit> = ours
            .hits
            .iter()
            .filter(|h| h.struck == Struck::Scenario("racer".into()))
            .collect();
        assert_eq!(racer_hits.len(), 1, "{:?}", ours.hits);
        let h = racer_hits[0];
        // The racer is the moving one; the police car was held stopped.
        assert!(
            !h.stopped && h.struck_mps > 15.0 && h.police_mps < 1.0 && h.closing_mps > 10.0,
            "{h:?}"
        );
        assert!(h.t_s > 1.0 && h.t_s < 2.0, "{h:?}");
        assert_eq!(ours.metrics["hits_moving_scenario"], Some(1.0));
        assert_eq!(ours.metrics["hits_stopped_scenario"], Some(0.0));
    }

    #[test]
    fn unknown_police_entity_is_an_error() {
        let sc = racer_into_stopped_cop();
        let data = Overlay::new(repo().join("data"));
        let err = run(&sc, &data, DEFAULT_STOPPED_MPS, false, &["nobody".into()]).unwrap_err();
        assert!(err.contains("nobody"), "{err}");
    }

    #[test]
    fn police_tables_come_from_the_overlay() {
        let sc = short_chase();
        let mut data = Overlay::new(repo().join("data"));
        let base = police_config(&data, &sc).unwrap();
        assert_eq!(
            base.pursuit.patrol_speed_factor,
            PoliceConfig::builtin().pursuit.patrol_speed_factor
        );
        let text = data
            .text("police/pursuit.toml")
            .unwrap()
            .replace("patrol_speed_factor = 1.0", "patrol_speed_factor = 0.5");
        data.files.insert("police/pursuit.toml".into(), text);
        let cfg = police_config(&data, &sc).unwrap();
        assert_eq!(cfg.pursuit.patrol_speed_factor, 0.5);
    }

    #[test]
    fn struck_serializes_as_a_label() {
        let v = serde_json::to_value(Struck::Scenario("racer".into())).unwrap();
        assert_eq!(v, serde_json::json!("scenario:racer"));
        assert_eq!(
            serde_json::to_value(Struck::World).unwrap(),
            serde_json::json!("world")
        );
    }
}
