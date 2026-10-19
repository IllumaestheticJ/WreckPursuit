//! # wp_wasm
//!
//! wasm-bindgen surface for the browser testbed (owned by workstream 04). A thin layer over
//! [`session::Session`], which is plain Rust and unit-tested natively. Hot per-frame data is
//! returned as packed typed arrays (layouts in [`pack`] and `docs/integration/testbed.md`);
//! JSON is used only for rare, structured calls (vehicle info, scenario results, status).
//!
//! Sessions are simcli scenarios (format v2): the sim is built by `simcli::run::build_sim`,
//! scripted entities use `simcli::run::driver_input`, results come from `simcli::run`, and
//! recordings are canonical `replay.bin` files that `simcli replay` verifies.
//!
//! | group | functions |
//! |---|---|
//! | session | `init(seed, scenario_json, scripted)`, `free_drive_json(car, mode, city)`, `scenario_from_ron(text)`, `scenario_with_speed(json, mps)`, `step(dt) -> alpha`, `set_damage_mode(str)`, `car_ids()` |
//! | control | `spawn_vehicle(car_id, pos, yaw)`, `set_input(entity, f32[])`, `set_assist_profile(entity, json)`, `assist_preset(source)`, `reset_vehicle(entity)` |
//! | state | `entities()`, `vehicle_states()`, `vehicle_states_prev()`, `vehicle_info(entity)`, `world_info()`, `state_hash()`, `traffic_agents()` |
//! | damage | `lattice_nodes`, `lattice_rest`, `lattice_beams`, `lattice_node_regions`, `lattice_strain`, `region_energy`, `skin_binding_indices`, `skin_binding_weights` |
//! | io | `events_drain() -> postcard`, `events_decode(bytes)`, `telemetry(entity)`, `telemetry_names()`, `ffb(entity)`, `intents()` |
//! | replay | `record_start()`, `record_stop()`, `recording()`, `replay_load(bytes)`, `replay_seek(tick)`, `replay_play(speed)`, `replay_status()`, `replay_export_csv(bytes, entity, every)` |
//! | scenario | `scenario_json()`, `scenario_result()` |

#![warn(missing_docs)]

pub mod data;
pub mod pack;
pub mod scenario;
pub mod session;

use std::cell::RefCell;

use serde::Serialize;
use wasm_bindgen::prelude::*;
use wp_contracts::{AssistProfile, DamageMode, EntityId, InputSource, SimEvent, PHYSICS_DT};

use crate::session::Session;

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}

fn err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&format!("wp_wasm: {e}"))
}

fn with<T>(f: impl FnOnce(&mut Session) -> Result<T, String>) -> Result<T, JsValue> {
    SESSION.with(|s| match s.borrow_mut().as_mut() {
        Some(s) => f(s).map_err(err),
        None => Err(err("call init() first")),
    })
}

fn get<T>(f: impl FnOnce(&mut Session) -> T) -> Result<T, JsValue> {
    with(|s| Ok(f(s)))
}

fn install(session: Session) -> Vec<u32> {
    let ids = session.entities().iter().map(|e| e.0).collect();
    SESSION.with(|s| *s.borrow_mut() = Some(session));
    ids
}

fn json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("serializable")
}

fn car_of(s: &Session, entity: u32) -> Result<&wp_contracts::LoadedCar, String> {
    s.car(EntityId(entity))
        .ok_or_else(|| format!("unknown entity {entity}"))
}

fn state_of(s: &Session, entity: u32) -> Result<wp_contracts::VehicleState, String> {
    s.sim()
        .vehicle_states()
        .into_iter()
        .find(|(id, _)| id.0 == entity)
        .map(|(_, st)| st)
        .ok_or_else(|| format!("unknown entity {entity}"))
}

// ----- session -----------------------------------------------------------------------------

/// Start a live session from a simcli v2 scenario as JSON (`scenario_from_ron`,
/// `free_drive_json`). `seed` non-zero overrides the scenario seed. `scripted` = drive the
/// entities by their scenario drivers until `duration_s` (menu scenarios); otherwise every
/// entity is live (free drive). Returns the entity ids in scenario order.
#[wasm_bindgen]
pub fn init(seed: u32, scenario_json: &str, scripted: bool) -> Result<Vec<u32>, JsValue> {
    let mut sc: simcli::Scenario = serde_json::from_str(scenario_json).map_err(err)?;
    if seed != 0 {
        sc.seed = seed as u64;
    }
    Ok(install(Session::new(sc, scripted).map_err(err)?))
}

/// Free-drive scenario JSON: one live `car` ("player"), `mode` (`"Arcade"` | `"Simulation"`),
/// optionally in the procedural 12×12 city (spawned on its race start grid, with traffic).
#[wasm_bindgen]
pub fn free_drive_json(car: &str, mode: &str, city: bool) -> Result<String, JsValue> {
    let mode = data::parse_mode(mode).map_err(err)?;
    Ok(json(&scenario::free_drive(car, mode, city)))
}

/// Parse a `.ron` scenario (simcli format v2) into JSON for `init`.
#[wasm_bindgen]
pub fn scenario_from_ron(text: &str) -> Result<String, JsValue> {
    scenario::from_ron(text).map(|s| json(&s)).map_err(err)
}

/// Override every `cruise` driver's target speed (m/s) in scenario JSON.
#[wasm_bindgen]
pub fn scenario_with_speed(scenario_json: &str, target_mps: f32) -> Result<String, JsValue> {
    let sc: simcli::Scenario = serde_json::from_str(scenario_json).map_err(err)?;
    Ok(json(&scenario::with_cruise_speed(sc, target_mps)))
}

/// Advance by `dt_seconds` of frame time: runs the whole fixed ticks it covers (at most 60)
/// and returns the leftover interpolation alpha `0..1` between `vehicle_states_prev()` and
/// `vehicle_states()`.
#[wasm_bindgen]
pub fn step(dt_seconds: f32) -> Result<f32, JsValue> {
    get(|s| s.step(dt_seconds))
}

/// Switch damage mode (`"Arcade"` | `"Simulation"`) without restarting; logged as a replay op.
#[wasm_bindgen]
pub fn set_damage_mode(mode: &str) -> Result<(), JsValue> {
    let mode = data::parse_mode(mode).map_err(err)?;
    with(|s| s.set_mode(mode))
}

/// JSON array of embedded car ids.
#[wasm_bindgen]
pub fn car_ids() -> String {
    json(&data::car_ids())
}

/// `wp_contracts::CONTRACT_VERSION`.
#[wasm_bindgen]
pub fn contract_version() -> String {
    wp_contracts::CONTRACT_VERSION.to_string()
}

// ----- control -----------------------------------------------------------------------------

/// Spawn `car_id` at world `pos` `[x, y, z]` with heading `yaw` (rad about +Z, 0 = +Y). Only
/// before the first tick (the spawn becomes part of the scenario setup). Returns the entity id.
#[wasm_bindgen]
pub fn spawn_vehicle(car_id: &str, pos: &[f32], yaw: f32) -> Result<u32, JsValue> {
    let p = std::array::from_fn(|i| pos.get(i).copied().unwrap_or(0.0));
    with(|s| s.spawn(car_id, p, yaw).map(|e| e.0))
}

/// Set a vehicle's input from a packed array
/// `[steer, throttle, brake, handbrake, clutch, flags, tool_use]` (see [`pack::unpack_input`]).
/// Returns `false` when ignored (unchanged, or a scripted scenario is running).
#[wasm_bindgen]
pub fn set_input(entity: u32, input: &[f32]) -> Result<bool, JsValue> {
    let input = pack::unpack_input(input);
    with(|s| s.set_input(EntityId(entity), input))
}

/// Set a vehicle's assists from `AssistProfile` JSON (logged as a replay op).
#[wasm_bindgen]
pub fn set_assist_profile(entity: u32, json_profile: &str) -> Result<(), JsValue> {
    let profile: AssistProfile = serde_json::from_str(json_profile).map_err(err)?;
    with(|s| s.set_assists(EntityId(entity), profile))
}

/// `AssistProfile` preset JSON for `"Keyboard"` | `"Gamepad"` | `"Wheel"`.
#[wasm_bindgen]
pub fn assist_preset(source: &str) -> Result<String, JsValue> {
    let source: InputSource =
        serde_json::from_value(serde_json::Value::String(source.into())).map_err(err)?;
    Ok(json(&AssistProfile::for_source(source)))
}

/// Current `AssistProfile` JSON of a vehicle.
#[wasm_bindgen]
pub fn assist_profile(entity: u32) -> Result<String, JsValue> {
    with(|s| {
        s.assists(EntityId(entity))
            .map(|p| json(&p))
            .ok_or_else(|| format!("unknown entity {entity}"))
    })
}

/// Return a vehicle to its spawn pose, pristine (logged as a replay op).
#[wasm_bindgen]
pub fn reset_vehicle(entity: u32) -> Result<(), JsValue> {
    with(|s| s.reset_vehicle(EntityId(entity)))
}

// ----- state -------------------------------------------------------------------------------

/// Entity ids in scenario order.
#[wasm_bindgen]
pub fn entities() -> Result<Vec<u32>, JsValue> {
    get(|s| s.entities().iter().map(|e| e.0).collect())
}

/// Floats per vehicle record in `vehicle_states()` ([`pack::STATE_STRIDE`]).
#[wasm_bindgen]
pub fn state_stride() -> u32 {
    pack::STATE_STRIDE as u32
}

/// Packed current state of every vehicle in scenario order
/// ([`pack::STATE_STRIDE`] floats each, see [`pack::pack_state`]).
#[wasm_bindgen]
pub fn vehicle_states() -> Result<Vec<f32>, JsValue> {
    get(|s| {
        let mut out = Vec::with_capacity(s.entities().len() * pack::STATE_STRIDE);
        for (id, st) in s.sim().vehicle_states() {
            pack::pack_state(id.0, &st, &mut out);
        }
        out
    })
}

/// Packed state of every vehicle *before the last tick* of the latest `step()` (same layout;
/// equals `vehicle_states()` for vehicles that have not ticked yet).
#[wasm_bindgen]
pub fn vehicle_states_prev() -> Result<Vec<f32>, JsValue> {
    get(|s| {
        let mut out = Vec::new();
        for (id, st) in s.sim().vehicle_states() {
            let prev = s
                .extra(id)
                .and_then(|x| x.prev_state.as_ref())
                .unwrap_or(&st);
            pack::pack_state(id.0, prev, &mut out);
        }
        out
    })
}

#[derive(Serialize)]
struct WheelInfo {
    position_m: [f32; 3],
    radius_m: f32,
    width_m: f32,
    steerable: bool,
    travel_m: f32,
    tire_id: String,
}

#[derive(Serialize)]
struct VehicleInfo {
    entity: u32,
    name: String,
    car_id: String,
    display_name: String,
    body_size_m: [f32; 3],
    mass_kg: f32,
    wheelbase_m: f32,
    cg_m: [f32; 3],
    redline_rpm: f32,
    rev_limit_rpm: f32,
    gears: usize,
    wheels: Vec<WheelInfo>,
    lattice_nodes: usize,
    lattice_beams: usize,
    skin_binds: usize,
    regions: Vec<String>,
    components: Vec<String>,
    mounted_components: Vec<String>,
    surfaces: Vec<String>,
    spawn_pos: [f32; 3],
}

/// Static info JSON for a vehicle: scenario name, dimensions, wheels, CG (chassis space),
/// lattice sizes and the name tables for region / component / surface indices.
#[wasm_bindgen]
pub fn vehicle_info(entity: u32) -> Result<String, JsValue> {
    with(|s| {
        let spec = s
            .spec(EntityId(entity))
            .ok_or_else(|| format!("unknown entity {entity}"))?;
        let car = car_of(s, entity)?;
        let d = &car.def;
        Ok(json(&VehicleInfo {
            entity,
            name: spec.id.clone(),
            car_id: d.id.clone(),
            display_name: d.display_name.clone(),
            body_size_m: d.body_size_m,
            mass_kg: d.mass_kg,
            wheelbase_m: d.wheelbase_m,
            cg_m: [
                0.0,
                d.wheelbase_m * (d.front_weight_frac - 0.5),
                d.cg_height_m,
            ],
            redline_rpm: d.engine.redline_rpm,
            rev_limit_rpm: d.engine.rev_limit_rpm,
            gears: d.drivetrain.gear_ratios.len(),
            wheels: d
                .wheels
                .iter()
                .zip(d.suspension.iter())
                .map(|(w, su)| WheelInfo {
                    position_m: w.position_m,
                    radius_m: w.radius_m,
                    width_m: w.width_m,
                    steerable: w.steerable,
                    travel_m: su.travel_m,
                    tire_id: w.tire_id.clone(),
                })
                .collect(),
            lattice_nodes: car.lattice.nodes.len(),
            lattice_beams: car.lattice.beams.len(),
            skin_binds: car.lattice.skin_binding.len(),
            regions: pack::ALL_REGIONS.iter().map(pack::variant_name).collect(),
            components: pack::ALL_COMPONENTS
                .iter()
                .map(pack::variant_name)
                .collect(),
            mounted_components: car
                .lattice
                .components
                .iter()
                .map(|c| pack::variant_name(&c.kind))
                .collect(),
            surfaces: pack::ALL_SURFACES.iter().map(pack::variant_name).collect(),
            spawn_pos: spec.pos,
        }))
    })
}

/// `[tick, time_of_day_h, night_factor, mode (0 Arcade / 1 Simulation), replay (0/1),
/// recording (0/1), city (0/1), scripted (0/1)]`.
#[wasm_bindgen]
pub fn world_info() -> Result<Vec<f32>, JsValue> {
    get(|s| {
        let t = s.sim().world().clock.time;
        let b = |v: bool| if v { 1.0 } else { 0.0 };
        vec![
            s.tick() as f32,
            t.hours(),
            t.night_factor(),
            b(s.mode() == DamageMode::Simulation),
            b(s.playback().is_some()),
            b(s.recording()),
            b(s.has_city()),
            b(s.scripted()),
        ]
    })
}

/// `Sim::state_hash()` as 16 hex digits.
#[wasm_bindgen]
pub fn state_hash() -> Result<String, JsValue> {
    get(|s| format!("{:016x}", s.sim().state_hash()))
}

/// Floats per agent in `traffic_agents()`.
pub const AGENT_STRIDE: usize = 5;

/// Civilian traffic agents (empty without a city), 5 floats each:
/// `[x, y, z, yaw, flags]` with flags = bits 1 physical LOD, 2 brake lights, 4 hazard lights,
/// 8 yielding to a siren, 16 turn signal left, 32 turn signal right, 64 aggressive driver,
/// plus `variant << 8`.
#[wasm_bindgen]
pub fn traffic_agents() -> Result<Vec<f32>, JsValue> {
    get(|s| {
        let mut out = Vec::with_capacity(s.sim().world().agent_count() * AGENT_STRIDE);
        for a in s.sim().world().agents() {
            let flags = (a.lod == wp_world::Lod::Physical) as u32
                | (a.brake_lights as u32) << 1
                | (a.hazard_lights as u32) << 2
                | (a.yielding as u32) << 3
                | ((a.turn_signal < 0) as u32) << 4
                | ((a.turn_signal > 0) as u32) << 5
                | ((a.personality == wp_world::traffic::Personality::Aggressive) as u32) << 6
                | (a.variant as u32) << 8;
            out.extend_from_slice(&[a.pos[0], a.pos[1], a.pos[2], a.yaw, flags as f32]);
        }
        out
    })
}

// ----- damage ------------------------------------------------------------------------------

fn nodes_of(s: &Session, entity: u32) -> Result<Vec<[f32; 3]>, String> {
    s.sim()
        .node_positions(EntityId(entity))
        .map(|n| n.to_vec())
        .map_err(|e| e.to_string())
}

/// Current lattice node positions in chassis space, `[x, y, z]` per node.
#[wasm_bindgen]
pub fn lattice_nodes(entity: u32) -> Result<Vec<f32>, JsValue> {
    with(|s| Ok(nodes_of(s, entity)?.concat()))
}

/// Rest-pose lattice node positions in chassis space, `[x, y, z]` per node.
#[wasm_bindgen]
pub fn lattice_rest(entity: u32) -> Result<Vec<f32>, JsValue> {
    with(|s| {
        Ok(car_of(s, entity)?
            .lattice
            .nodes
            .iter()
            .flat_map(|n| n.pos_m)
            .collect())
    })
}

/// Beam node pairs `[a, b]` per beam.
#[wasm_bindgen]
pub fn lattice_beams(entity: u32) -> Result<Vec<u32>, JsValue> {
    with(|s| {
        Ok(car_of(s, entity)?
            .lattice
            .beams
            .iter()
            .flat_map(|b| [b.a as u32, b.b as u32])
            .collect())
    })
}

/// Region index (into `vehicle_info().regions`) per node.
#[wasm_bindgen]
pub fn lattice_node_regions(entity: u32) -> Result<Vec<u8>, JsValue> {
    with(|s| {
        Ok(car_of(s, entity)?
            .lattice
            .nodes
            .iter()
            .map(|n| pack::region_index(n.region) as u8)
            .collect())
    })
}

/// Per-node stress proxy: max |engineering strain| of incident beams.
#[wasm_bindgen]
pub fn lattice_strain(entity: u32) -> Result<Vec<f32>, JsValue> {
    with(|s| {
        let nodes = nodes_of(s, entity)?;
        Ok(pack::node_strain(&car_of(s, entity)?.lattice, &nodes))
    })
}

/// Deformation energy per body region (J), indexed like `vehicle_info().regions`.
#[wasm_bindgen]
pub fn region_energy(entity: u32) -> Result<Vec<f32>, JsValue> {
    with(|s| {
        let nodes = nodes_of(s, entity)?;
        let car = car_of(s, entity)?;
        Ok(pack::region_energy(&car.lattice, &car.materials, &nodes))
    })
}

/// Skin binding indices: `[vertex_index, node0, node1, node2, node3]` per bind (empty for
/// cars without a visual mesh).
#[wasm_bindgen]
pub fn skin_binding_indices(entity: u32) -> Result<Vec<u32>, JsValue> {
    with(|s| {
        Ok(car_of(s, entity)?
            .lattice
            .skin_binding
            .iter()
            .flat_map(|b| {
                [
                    b.vertex_index,
                    b.node_ids[0] as u32,
                    b.node_ids[1] as u32,
                    b.node_ids[2] as u32,
                    b.node_ids[3] as u32,
                ]
            })
            .collect())
    })
}

/// Skin binding weights: 4 per bind, same order as `skin_binding_indices`.
#[wasm_bindgen]
pub fn skin_binding_weights(entity: u32) -> Result<Vec<f32>, JsValue> {
    with(|s| {
        Ok(car_of(s, entity)?
            .lattice
            .skin_binding
            .iter()
            .flat_map(|b| b.weights)
            .collect())
    })
}

// ----- io ----------------------------------------------------------------------------------

/// Take all events since the last drain, encoded as postcard `Vec<SimEvent>`.
#[wasm_bindgen]
pub fn events_drain() -> Result<Vec<u8>, JsValue> {
    get(|s| postcard::to_allocvec(&s.drain_events()).expect("events serialize"))
}

/// Decode `events_drain()` bytes to JSON (serde externally-tagged `SimEvent`s).
#[wasm_bindgen]
pub fn events_decode(bytes: &[u8]) -> Result<String, JsValue> {
    let events: Vec<SimEvent> = postcard::from_bytes(bytes).map_err(err)?;
    Ok(json(&events))
}

/// Telemetry record for a vehicle; channel names from `telemetry_names()`.
#[wasm_bindgen]
pub fn telemetry(entity: u32) -> Result<Vec<f32>, JsValue> {
    with(|s| telemetry_record(s, entity))
}

fn telemetry_record(s: &Session, entity: u32) -> Result<Vec<f32>, String> {
    let st = state_of(s, entity)?;
    let id = EntityId(entity);
    let accel = s.extra(id).map_or([0.0; 3], |x| x.accel_local);
    Ok(pack::pack_telemetry(&st, &s.input(id), accel))
}

/// JSON array of telemetry channel names.
#[wasm_bindgen]
pub fn telemetry_names() -> String {
    json(&pack::telemetry_names())
}

/// Force feedback `[rack_torque_nm, road_rumble, damper]`. The browser has no FFB device;
/// the testbed draws the rack torque as a bar.
#[wasm_bindgen]
pub fn ffb(entity: u32) -> Result<Vec<f32>, JsValue> {
    with(|s| {
        s.ffb(EntityId(entity))
            .map(|f| vec![f.rack_torque_nm, f.road_rumble, f.damper])
            .ok_or_else(|| format!("unknown entity {entity}"))
    })
}

/// Traffic / police intent arrows, 8 floats each: `[entity, kind, from xyz, to xyz]`
/// (kind 0 = traffic route, 1 = police target). **Placeholder hook**: empty until the
/// lead exposes intents (request TB-5, deferred).
#[wasm_bindgen]
pub fn intents() -> Result<Vec<f32>, JsValue> {
    get(|_| Vec::new())
}

// ----- replay ------------------------------------------------------------------------------

/// Mark recording as active (the session log always covers the whole session from tick 0).
#[wasm_bindgen]
pub fn record_start() -> Result<(), JsValue> {
    get(|s| s.record_start())
}

/// Stop recording; returns canonical `replay.bin` bytes (simcli replay format v2).
#[wasm_bindgen]
pub fn record_stop() -> Result<Vec<u8>, JsValue> {
    with(|s| s.record_stop().map(|r| r.encode()))
}

/// Whether a recording is in progress.
#[wasm_bindgen]
pub fn recording() -> Result<bool, JsValue> {
    get(|s| s.recording())
}

/// Load `replay.bin` bytes and switch to playback (live control disabled until `init`).
/// Returns the entity ids.
#[wasm_bindgen]
pub fn replay_load(bytes: &[u8]) -> Result<Vec<u32>, JsValue> {
    Ok(install(Session::from_replay_bytes(bytes).map_err(err)?))
}

/// Seek playback to `tick` (backwards seeks re-simulate from tick 0).
#[wasm_bindgen]
pub fn replay_seek(tick: f64) -> Result<(), JsValue> {
    with(|s| s.replay_seek(tick.max(0.0) as u64))
}

/// Set playback speed multiplier (0 = pause, max 16).
#[wasm_bindgen]
pub fn replay_play(speed: f32) -> Result<(), JsValue> {
    with(|s| s.replay_play(speed))
}

/// Playback status JSON (`null` when live): tick, end, speed, verified / drifted ticks,
/// current and recorded final hash.
#[wasm_bindgen]
pub fn replay_status() -> Result<String, JsValue> {
    get(|s| json(&s.replay_status()))
}

/// Re-simulate a replay headless and export `entity`'s telemetry every `every_n_ticks`
/// ticks as CSV with a `tick,t_s,...` header.
#[wasm_bindgen]
pub fn replay_export_csv(bytes: &[u8], entity: u32, every_n_ticks: u32) -> Result<String, JsValue> {
    export_csv(bytes, entity, every_n_ticks.max(1) as u64).map_err(err)
}

/// Native-testable body of [`replay_export_csv`].
pub fn export_csv(bytes: &[u8], entity: u32, every: u64) -> Result<String, String> {
    let mut s = Session::from_replay_bytes(bytes)?;
    let end = s.playback().map_or(0, |p| p.replay.ticks);
    let mut csv = String::from("tick,t_s,");
    csv.push_str(&pack::telemetry_names().join(","));
    csv.push('\n');
    loop {
        let tick = s.tick();
        let rec = telemetry_record(&s, entity)?;
        csv.push_str(&format!("{tick},{:.5}", tick as f32 * PHYSICS_DT));
        for v in rec {
            csv.push_str(&format!(",{v}"));
        }
        csv.push('\n');
        if tick >= end {
            break;
        }
        s.run_ticks(every.min(end - tick));
    }
    Ok(csv)
}

// ----- scenario ----------------------------------------------------------------------------

/// The session's scenario as JSON (simcli v2; entities include tick-0 spawns, `barriers` for
/// drawing).
#[wasm_bindgen]
pub fn scenario_json() -> Result<String, JsValue> {
    get(|s| json(s.scenario()))
}

/// Scripted-scenario progress / outcome JSON (`null` for free drive or replays): once done,
/// the headless simcli summary (all expectation kinds), the live end hash and whether it
/// matches simcli's.
#[wasm_bindgen]
pub fn scenario_result() -> Result<String, JsValue> {
    get(|s| json(&s.scenario_result()))
}

#[cfg(test)]
mod tests;
