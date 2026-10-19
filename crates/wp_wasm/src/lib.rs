//! # wp_wasm
//!
//! wasm-bindgen surface for the browser testbed (owned by workstream 04). A thin layer over
//! [`session::Session`], which is plain Rust and unit-tested natively. Hot per-frame data is
//! returned as packed typed arrays (layouts in [`pack`] and `docs/integration/testbed.md`);
//! JSON is used only for rare, structured calls (vehicle info, scenario results, status).
//!
//! Everything goes through the `wp_contracts` traits (`VehicleSim`, `DamageSim`) via
//! `wp_sim::Sim`, so when the real vehicle/damage crates replace the stubs nothing here
//! changes.
//!
//! | group | functions |
//! |---|---|
//! | session | `init(seed, scenario_json)`, `scenario_from_ron(text)`, `step(dt) -> alpha`, `set_damage_mode(str)`, `car_ids()` |
//! | control | `spawn_vehicle(car_id, pos, yaw)`, `set_input(entity, f32[])`, `set_assist_profile(entity, json)`, `assist_preset(source)`, `reset_vehicle(entity)` |
//! | state | `entities()`, `vehicle_states()`, `vehicle_states_prev()`, `vehicle_info(entity)`, `world_info()`, `state_hash()` |
//! | damage | `lattice_nodes`, `lattice_rest`, `lattice_beams`, `lattice_node_regions`, `lattice_strain`, `region_energy`, `skin_binding_indices`, `skin_binding_weights` |
//! | io | `events_drain() -> postcard`, `events_decode(bytes)`, `telemetry(entity)`, `telemetry_names()`, `ffb(entity)`, `intents()` |
//! | replay | `record_start()`, `record_stop()`, `recording()`, `replay_load(bytes)`, `replay_seek(tick)`, `replay_play(speed)`, `replay_status()`, `replay_export_csv(bytes, entity, every)` |
//! | scenario | `scenario_json()`, `scenario_result()` |

#![warn(missing_docs)]

pub mod data;
pub mod pack;
pub mod replay;
pub mod scenario;
pub mod session;

use std::cell::RefCell;

use serde::Serialize;
use wasm_bindgen::prelude::*;
use wp_contracts::{AssistProfile, EntityId, InputSource, SimEvent};

use crate::replay::Replay;
use crate::scenario::Scenario;
use crate::session::Session;

struct App {
    session: Session,
    /// Scenario and seed the live session was built from (for `set_damage_mode` and leaving
    /// replay mode).
    scenario: Scenario,
    seed: u64,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&format!("wp_wasm: {e}"))
}

fn with_app<T>(f: impl FnOnce(&mut App) -> Result<T, String>) -> Result<T, JsValue> {
    APP.with(|app| match app.borrow_mut().as_mut() {
        Some(a) => f(a).map_err(err),
        None => Err(err("call init() first")),
    })
}

fn with_session<T>(f: impl FnOnce(&mut Session) -> T) -> Result<T, JsValue> {
    with_app(|a| Ok(f(&mut a.session)))
}

fn json<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).expect("serializable")
}

fn extra<T>(
    s: &Session,
    entity: u32,
    f: impl FnOnce(&session::VehicleExtra) -> T,
) -> Result<T, String> {
    s.extra(EntityId(entity))
        .map(f)
        .ok_or_else(|| format!("unknown entity {entity}"))
}

// ----- session -----------------------------------------------------------------------------

/// Start a fresh live session. `seed` (0 = use the scenario's) and `scenario_json` (a
/// [`Scenario`] as JSON; empty = free drive with one `debug_sedan`). Returns the entity ids.
#[wasm_bindgen]
pub fn init(seed: u32, scenario_json: &str) -> Result<Vec<u32>, JsValue> {
    let scenario: Scenario = if scenario_json.trim().is_empty() {
        Scenario::default()
    } else {
        serde_json::from_str(scenario_json).map_err(err)?
    };
    let session = Session::from_scenario(seed as u64, scenario.clone()).map_err(err)?;
    let ids = session.entities().iter().map(|e| e.0).collect();
    APP.with(|a| {
        *a.borrow_mut() = Some(App {
            session,
            scenario,
            seed: seed as u64,
        })
    });
    Ok(ids)
}

/// Parse a `.ron` scenario (simcli format + testbed extensions) and return it as JSON for
/// `init`.
#[wasm_bindgen]
pub fn scenario_from_ron(text: &str) -> Result<String, JsValue> {
    Scenario::from_ron(text).map(|s| json(&s)).map_err(err)
}

/// Advance by `dt_seconds` of frame time: runs the whole fixed ticks it covers (at most 60)
/// and returns the leftover interpolation alpha `0..1` between `vehicle_states_prev()` and
/// `vehicle_states()`.
#[wasm_bindgen]
pub fn step(dt_seconds: f32) -> Result<f32, JsValue> {
    with_session(|s| s.step(dt_seconds))
}

/// Switch damage mode (`"Arcade"` | `"Simulation"`). **Restarts the session** with the same
/// seed and scenario (`wp_sim` cannot swap mode tables mid-run; request TB-4).
#[wasm_bindgen]
pub fn set_damage_mode(mode: &str) -> Result<Vec<u32>, JsValue> {
    let mode = data::parse_mode(mode).map_err(err)?;
    let (mut scenario, seed) = with_app(|a| Ok((a.scenario.clone(), a.seed)))?;
    scenario.mode = mode;
    init(seed as u32, &json(&scenario))
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

/// Spawn `car_id` at world `pos` `[x, y, z]` with heading `yaw` (rad about +Z, 0 = +Y).
/// Returns the entity id.
#[wasm_bindgen]
pub fn spawn_vehicle(car_id: &str, pos: &[f32], yaw: f32) -> Result<u32, JsValue> {
    let p = [
        pos.first().copied().unwrap_or(0.0),
        pos.get(1).copied().unwrap_or(0.0),
        pos.get(2).copied().unwrap_or(0.0),
    ];
    with_app(|a| a.session.spawn(car_id, p, yaw).map(|e| e.0))
}

/// Set a vehicle's input from a packed array
/// `[steer, throttle, brake, handbrake, clutch, flags, tool_use]` (see [`pack::unpack_input`]).
/// Returns `false` when ignored (unchanged, or the vehicle is scenario-scripted).
#[wasm_bindgen]
pub fn set_input(entity: u32, input: &[f32]) -> Result<bool, JsValue> {
    let input = pack::unpack_input(input);
    with_app(|a| a.session.set_input(EntityId(entity), input))
}

/// Set a vehicle's assists from `AssistProfile` JSON.
#[wasm_bindgen]
pub fn set_assist_profile(entity: u32, json_profile: &str) -> Result<(), JsValue> {
    let profile: AssistProfile = serde_json::from_str(json_profile).map_err(err)?;
    with_app(|a| a.session.set_assists(EntityId(entity), profile))
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
    with_app(|a| {
        a.session
            .assists(EntityId(entity))
            .map(|p| json(&p))
            .ok_or_else(|| format!("unknown entity {entity}"))
    })
}

/// Return a vehicle to its spawn pose, pristine.
#[wasm_bindgen]
pub fn reset_vehicle(entity: u32) -> Result<(), JsValue> {
    with_app(|a| a.session.reset_vehicle(EntityId(entity)))
}

// ----- state -------------------------------------------------------------------------------

/// Entity ids, ascending.
#[wasm_bindgen]
pub fn entities() -> Result<Vec<u32>, JsValue> {
    with_session(|s| s.entities().iter().map(|e| e.0).collect())
}

/// Floats per vehicle record in `vehicle_states()` ([`pack::STATE_STRIDE`]).
#[wasm_bindgen]
pub fn state_stride() -> u32 {
    pack::STATE_STRIDE as u32
}

/// Packed current state of every vehicle, ascending by entity
/// ([`pack::STATE_STRIDE`] floats each, see [`pack::pack_state`]).
#[wasm_bindgen]
pub fn vehicle_states() -> Result<Vec<f32>, JsValue> {
    with_session(|s| {
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
    with_session(|s| {
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

/// Static info JSON for a vehicle: dimensions, wheels, CG (chassis space), lattice sizes and
/// the name tables for region / component / surface indices.
#[wasm_bindgen]
pub fn vehicle_info(entity: u32) -> Result<String, JsValue> {
    with_app(|a| {
        extra(&a.session, entity, |x| {
            let d = &x.car.def;
            let info = VehicleInfo {
                entity,
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
                    .map(|(w, s)| WheelInfo {
                        position_m: w.position_m,
                        radius_m: w.radius_m,
                        width_m: w.width_m,
                        steerable: w.steerable,
                        travel_m: s.travel_m,
                        tire_id: w.tire_id.clone(),
                    })
                    .collect(),
                lattice_nodes: x.car.lattice.nodes.len(),
                lattice_beams: x.car.lattice.beams.len(),
                skin_binds: x.car.lattice.skin_binding.len(),
                regions: pack::ALL_REGIONS.iter().map(pack::variant_name).collect(),
                components: pack::ALL_COMPONENTS
                    .iter()
                    .map(pack::variant_name)
                    .collect(),
                mounted_components: x
                    .car
                    .lattice
                    .components
                    .iter()
                    .map(|c| pack::variant_name(&c.kind))
                    .collect(),
                surfaces: pack::ALL_SURFACES.iter().map(pack::variant_name).collect(),
                spawn_pos: x.spawn_pos,
            };
            json(&info)
        })
    })
}

/// `[tick, time_of_day_hours, night_factor, mode (0 Arcade / 1 Simulation), replay (0/1),
/// recording (0/1)]`.
#[wasm_bindgen]
pub fn world_info() -> Result<Vec<f32>, JsValue> {
    with_session(|s| {
        let t = s.sim().world().clock.time;
        vec![
            s.tick() as f32,
            t.hours(),
            t.night_factor(),
            match s.mode() {
                wp_contracts::DamageMode::Arcade => 0.0,
                wp_contracts::DamageMode::Simulation => 1.0,
            },
            if s.playback().is_some() { 1.0 } else { 0.0 },
            if s.recording() { 1.0 } else { 0.0 },
        ]
    })
}

/// `Sim::state_hash()` as 16 hex digits.
#[wasm_bindgen]
pub fn state_hash() -> Result<String, JsValue> {
    with_session(|s| format!("{:016x}", s.sim().state_hash()))
}

// ----- damage ------------------------------------------------------------------------------

fn nodes_of(s: &Session, entity: u32) -> Result<Vec<[f32; 3]>, String> {
    s.lattice_nodes(EntityId(entity))
        .map(|n| n.to_vec())
        .ok_or_else(|| format!("unknown entity {entity}"))
}

/// Current lattice node positions in chassis space, `[x, y, z]` per node.
#[wasm_bindgen]
pub fn lattice_nodes(entity: u32) -> Result<Vec<f32>, JsValue> {
    with_app(|a| Ok(nodes_of(&a.session, entity)?.concat()))
}

/// Rest-pose lattice node positions in chassis space, `[x, y, z]` per node.
#[wasm_bindgen]
pub fn lattice_rest(entity: u32) -> Result<Vec<f32>, JsValue> {
    with_app(|a| {
        extra(&a.session, entity, |x| {
            x.car.lattice.nodes.iter().flat_map(|n| n.pos_m).collect()
        })
    })
}

/// Beam node pairs `[a, b]` per beam.
#[wasm_bindgen]
pub fn lattice_beams(entity: u32) -> Result<Vec<u32>, JsValue> {
    with_app(|a| {
        extra(&a.session, entity, |x| {
            x.car
                .lattice
                .beams
                .iter()
                .flat_map(|b| [b.a as u32, b.b as u32])
                .collect()
        })
    })
}

/// Region index (into `vehicle_info().regions`) per node.
#[wasm_bindgen]
pub fn lattice_node_regions(entity: u32) -> Result<Vec<u8>, JsValue> {
    with_app(|a| {
        extra(&a.session, entity, |x| {
            x.car
                .lattice
                .nodes
                .iter()
                .map(|n| pack::region_index(n.region) as u8)
                .collect()
        })
    })
}

/// Per-node stress proxy: max |engineering strain| of incident beams.
#[wasm_bindgen]
pub fn lattice_strain(entity: u32) -> Result<Vec<f32>, JsValue> {
    with_app(|a| {
        let nodes = nodes_of(&a.session, entity)?;
        extra(&a.session, entity, |x| {
            pack::node_strain(&x.car.lattice, &nodes)
        })
    })
}

/// Deformation energy per body region (J), indexed like `vehicle_info().regions`.
#[wasm_bindgen]
pub fn region_energy(entity: u32) -> Result<Vec<f32>, JsValue> {
    with_app(|a| {
        let nodes = nodes_of(&a.session, entity)?;
        extra(&a.session, entity, |x| {
            pack::region_energy(&x.car.lattice, &x.car.materials, &nodes)
        })
    })
}

/// Skin binding indices: `[vertex_index, node0, node1, node2, node3]` per bind (empty for
/// debug cars without a visual mesh).
#[wasm_bindgen]
pub fn skin_binding_indices(entity: u32) -> Result<Vec<u32>, JsValue> {
    with_app(|a| {
        extra(&a.session, entity, |x| {
            x.car
                .lattice
                .skin_binding
                .iter()
                .flat_map(|s| {
                    [
                        s.vertex_index,
                        s.node_ids[0] as u32,
                        s.node_ids[1] as u32,
                        s.node_ids[2] as u32,
                        s.node_ids[3] as u32,
                    ]
                })
                .collect()
        })
    })
}

/// Skin binding weights: 4 per bind, same order as `skin_binding_indices`.
#[wasm_bindgen]
pub fn skin_binding_weights(entity: u32) -> Result<Vec<f32>, JsValue> {
    with_app(|a| {
        extra(&a.session, entity, |x| {
            x.car
                .lattice
                .skin_binding
                .iter()
                .flat_map(|s| s.weights)
                .collect()
        })
    })
}

// ----- io ----------------------------------------------------------------------------------

/// Take all events since the last drain, encoded as postcard `Vec<SimEvent>`.
#[wasm_bindgen]
pub fn events_drain() -> Result<Vec<u8>, JsValue> {
    with_session(|s| postcard::to_allocvec(&s.drain_events()).expect("events serialize"))
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
    with_app(|a| {
        let s = &a.session;
        let state = s
            .sim()
            .vehicle_states()
            .into_iter()
            .find(|(id, _)| id.0 == entity)
            .map(|(_, st)| st)
            .ok_or_else(|| format!("unknown entity {entity}"))?;
        let input = s.input(EntityId(entity));
        extra(s, entity, |x| {
            pack::pack_telemetry(&x.car, &state, &input, x.accel_local)
        })
    })
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
    with_app(|a| {
        a.session
            .ffb(EntityId(entity))
            .map(|f| vec![f.rack_torque_nm, f.road_rumble, f.damper])
            .ok_or_else(|| format!("unknown entity {entity}"))
    })
}

/// Traffic / police intent arrows, 8 floats each: `[entity, kind, from xyz, to xyz]`
/// (kind 0 = traffic route, 1 = police target). **Placeholder hook**: empty until
/// `wp_world`/`wp_police` expose intents (request TB-5).
#[wasm_bindgen]
pub fn intents() -> Result<Vec<f32>, JsValue> {
    with_session(|_| Vec::new())
}

// ----- replay ------------------------------------------------------------------------------

/// Mark the start of a recording (the op log always covers the whole session, so earlier
/// ticks can be reproduced).
#[wasm_bindgen]
pub fn record_start() -> Result<(), JsValue> {
    with_session(|s| s.record_start())
}

/// Stop recording; returns the replay bytes (see [`replay::Replay`]).
#[wasm_bindgen]
pub fn record_stop() -> Result<Vec<u8>, JsValue> {
    with_session(|s| s.record_stop().to_bytes())
}

/// Whether a recording is in progress.
#[wasm_bindgen]
pub fn recording() -> Result<bool, JsValue> {
    with_session(|s| s.recording())
}

/// Load a replay and switch the session to playback (live control disabled until `init`).
/// Returns the entity ids present at the view start.
#[wasm_bindgen]
pub fn replay_load(bytes: &[u8]) -> Result<Vec<u32>, JsValue> {
    let replay = Replay::from_bytes(bytes).map_err(err)?;
    let session = Session::from_replay(replay).map_err(err)?;
    let ids = session.entities().iter().map(|e| e.0).collect();
    APP.with(|a| {
        let mut a = a.borrow_mut();
        let (scenario, seed) = a
            .as_ref()
            .map(|x| (x.scenario.clone(), x.seed))
            .unwrap_or_default();
        *a = Some(App {
            session,
            scenario,
            seed,
        });
    });
    Ok(ids)
}

/// Seek playback to `tick` (backwards seeks re-simulate from tick 0).
#[wasm_bindgen]
pub fn replay_seek(tick: f64) -> Result<(), JsValue> {
    with_app(|a| a.session.replay_seek(tick.max(0.0) as u64))
}

/// Set playback speed multiplier (0 = pause, max 16).
#[wasm_bindgen]
pub fn replay_play(speed: f32) -> Result<(), JsValue> {
    with_app(|a| a.session.replay_play(speed))
}

/// Playback status JSON (`null` when live): tick range, speed, verified / drifted hashes,
/// current and recorded final hash.
#[wasm_bindgen]
pub fn replay_status() -> Result<String, JsValue> {
    with_session(|s| json(&s.replay_status()))
}

/// Re-simulate a replay headless and export `entity`'s telemetry every `every_n_ticks`
/// ticks (from the view start) as CSV with a `tick,t_s,...` header.
#[wasm_bindgen]
pub fn replay_export_csv(bytes: &[u8], entity: u32, every_n_ticks: u32) -> Result<String, JsValue> {
    let replay = Replay::from_bytes(bytes).map_err(err)?;
    export_csv(replay, entity, every_n_ticks.max(1) as u64).map_err(err)
}

/// Native-testable body of [`replay_export_csv`].
pub fn export_csv(replay: Replay, entity: u32, every: u64) -> Result<String, String> {
    let end = replay.end_tick;
    let mut s = Session::from_replay(replay)?;
    let mut csv = String::from("tick,t_s,");
    csv.push_str(&pack::telemetry_names().join(","));
    csv.push('\n');
    loop {
        let tick = s.tick();
        if let Some(x) = s.extra(EntityId(entity)) {
            if let Some((_, st)) = s
                .sim()
                .vehicle_states()
                .into_iter()
                .find(|(id, _)| id.0 == entity)
            {
                let rec =
                    pack::pack_telemetry(&x.car, &st, &s.input(EntityId(entity)), x.accel_local);
                csv.push_str(&format!(
                    "{tick},{:.5}",
                    tick as f32 * wp_contracts::PHYSICS_DT
                ));
                for v in rec {
                    csv.push_str(&format!(",{v}"));
                }
                csv.push('\n');
            }
        }
        if tick >= end {
            break;
        }
        s.run_ticks(every.min(end - tick));
    }
    Ok(csv)
}

// ----- scenario ----------------------------------------------------------------------------

/// The scenario JSON the live session was built from (barriers etc. for drawing).
#[wasm_bindgen]
pub fn scenario_json() -> Result<String, JsValue> {
    with_app(|a| Ok(json(&a.scenario)))
}

/// Scenario progress / pass-fail JSON (`null` for free drive or replays).
#[wasm_bindgen]
pub fn scenario_result() -> Result<String, JsValue> {
    with_session(|s| json(&s.scenario_result()))
}

#[cfg(test)]
mod tests;
