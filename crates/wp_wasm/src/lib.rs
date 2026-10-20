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
//! | session | `init(seed, scenario_json, scripted)`, `free_drive_json(car, mode, city)`, `free_drive_city_json(car, mode, city_id)`, `city_chase_city_json(car, mode, city_id)`, `city_id()`, `scenario_from_ron(text)`, `scenario_with_speed(json, mps)`, `step(dt) -> alpha`, `set_damage_mode(str)`, `car_ids()` |
//! | control | `spawn_vehicle(car_id, pos, yaw)`, `set_input(entity, f32[])`, `set_assist_profile(entity, json)`, `assist_preset(source)`, `set_countersteer(entity, %)` / `countersteer(entity)`, `set_clutch_mode(entity, "auto"|"manual")` / `clutch_mode(entity)`, `engine_stalled(entity)`, `reset_vehicle(entity)` |
//! | state | `entities()`, `vehicle_states()`, `vehicle_states_prev()`, `vehicle_info(entity)`, `world_info()`, `state_hash()`, `traffic_agents()`, `signal_heads()`, `signal_states()` |
//! | damage | `lattice_nodes`, `lattice_rest`, `lattice_beams`, `lattice_node_regions`, `lattice_strain`, `region_energy`, `skin_binding_indices`, `skin_binding_weights` |
//! | io | `events_drain() -> postcard`, `events_decode(bytes)`, `telemetry(entity)`, `telemetry_names()`, `ffb(entity)`, `intents()` |
//! | replay | `record_start()`, `record_stop()`, `recording()`, `replay_load(bytes)`, `replay_seek(tick)`, `replay_play(speed)`, `replay_status()`, `replay_export_csv(bytes, entity, every)` |
//! | scenario | `scenario_json()`, `scenario_result()` |
//! | race (15) | `race_list(car, mode)`, `race_start(id, car, mode, autopilot)`, `race_info()`, `race_standings()`, `race_course()`, `race_result_json()` — see [`race`] |
//! | run (lead) | `run_result_json()`, `run_ledger_json()`, `run_account_json()`, `run_repair_continue()`, `debug_set_heat(entity, heat)`, `debug_park_near_police(entity, ahead_m)` — see [`run`] |
//! | map (16) | `map_static_json()` once per city, `map_dynamic(player)` per frame — see [`map`] |
//! | weather (lead) | `weather_choices()`, `scenario_with_weather(json, choice)`, `scenario_weather_choice(json)`, `city_chase_rain_json(car, mode, city_id)`, `weather_info()`, `weather_grid_meta()`, `weather_grid()`, `weather_spray()` — see [`weather`] |
//! | career (G) | `career_new(mode)`, `career_load_json`, `career_save_json`, `career_view_json`, `career_mods_json(uid)`, `career_start_run(scenario_json)`, `career_end_run`, `career_run_active`, `career_buy_car`, `career_floor_car`, `career_sell_car`, `career_set_active`, `career_install_mod`, `career_uninstall_mod`, `career_quote_json`, `career_repair`, `career_release`, `career_insure`, `career_next_night` — see [`career`] |
//! | audio (13) | `audio_library()`, `audio_frame(listener, events_json, focus)`, `audio_set_siren`, `audio_set_scanner_tier`, `audio_radio`, `audio_reset()` — see [`audio`] |

#![warn(missing_docs)]

/// Multiplayer client surface (`net_*` functions, workstream F), see the module docs.
pub mod net;

pub mod audio;
pub mod career;
pub mod data;
pub mod garage;
pub mod lod;
pub mod map;
pub mod pack;
pub mod police;
pub mod race;
pub mod run;
pub mod scenario;
pub mod session;
pub mod signals;
pub mod weather;

#[cfg(test)]
mod career_tests;
#[cfg(test)]
mod race_tests;
#[cfg(test)]
mod run_tests;

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

/// Run `f` with the current session, if any (career glue: works with no session too).
fn with_session_opt<T, E>(f: impl FnOnce(Option<&mut Session>) -> Result<T, E>) -> Result<T, E> {
    SESSION.with(|s| f(s.borrow_mut().as_mut()))
}

fn get<T>(f: impl FnOnce(&mut Session) -> T) -> Result<T, JsValue> {
    with(|s| Ok(f(s)))
}

fn install(mut session: Session) -> Vec<u32> {
    let ids = session.entities().iter().map(|e| e.0).collect();
    SESSION.with(|s| {
        let mut slot = s.borrow_mut();
        if let Some(prev) = slot.as_mut() {
            if prev.is_career() {
                // A career run being replaced ends: the career absorbs it (machine G).
                career::absorb(prev);
            } else if let Some(run) = prev.take_run() {
                // The player's account (cash, garage, ledger) carries over a restart.
                if !session.is_career() {
                    session.adopt_account(run);
                }
            }
        }
        *slot = Some(session);
    });
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

/// "City chase" preset JSON (live): Port Vesper v2 at night, full police presence, traffic,
/// the player already speeding on the race grid.
#[wasm_bindgen]
pub fn city_chase_json(car: &str, mode: &str) -> Result<String, JsValue> {
    let mode = data::parse_mode(mode).map_err(err)?;
    Ok(json(&scenario::city_chase(car, mode)))
}

/// Free drive in a chosen city: `city_id` = `"vesper_v2"` (Port Vesper v2) or
/// `"procedural_12x12"` (the old grid); anything else = flat world.
#[wasm_bindgen]
pub fn free_drive_city_json(car: &str, mode: &str, city_id: &str) -> Result<String, JsValue> {
    let mode = data::parse_mode(mode).map_err(err)?;
    Ok(json(&scenario::free_drive_in(
        car,
        mode,
        scenario::city_spec(city_id),
    )))
}

/// City chase in a chosen city (`city_id` as in [`free_drive_city_json`]).
#[wasm_bindgen]
pub fn city_chase_city_json(car: &str, mode: &str, city_id: &str) -> Result<String, JsValue> {
    let mode = data::parse_mode(mode).map_err(err)?;
    Ok(json(&scenario::city_chase_in(
        car,
        mode,
        scenario::city_spec(city_id),
    )))
}

/// Id of the session's city (`vesper_v2`, `procedural_12x12`, a file stem) or `""` on the
/// flat world: the testbed loads `data/city/<id>.greybox.glb`.
#[wasm_bindgen]
pub fn city_id() -> Result<String, JsValue> {
    get(|s| {
        if s.has_city() {
            s.sim().world().network().id.clone()
        } else {
            String::new()
        }
    })
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

/// Countersteer help slider (owner row 57): `percent` 0..=100 sets the vehicle's
/// `AssistProfile::countersteer_help` (0 = plain return to centre, 50 ≈ Wreckfest default,
/// 100 = fast, full alignment of the front wheels with the car's travel; see
/// `wp_vehicle::steering::steer_target`). Logged as a replay op like any assist change.
#[wasm_bindgen]
pub fn set_countersteer(entity: u32, percent: f32) -> Result<(), JsValue> {
    with(|s| {
        let id = EntityId(entity);
        let mut p = s
            .assists(id)
            .ok_or_else(|| format!("unknown entity {entity}"))?;
        p.countersteer_help = (percent / 100.0).clamp(0.0, 1.0);
        s.set_assists(id, p)
    })
}

/// Countersteer help slider of a vehicle, 0..=100 %.
#[wasm_bindgen]
pub fn countersteer(entity: u32) -> Result<f32, JsValue> {
    with(|s| {
        s.assists(EntityId(entity))
            .map(|p| p.countersteer_help * 100.0)
            .ok_or_else(|| format!("unknown entity {entity}"))
    })
}

/// Clutch mode (owner row 59): `"auto"` (automatic clutch with anti-stall; the clutch input is
/// ignored except to open it) or `"manual"` (the clutch input is the pedal: it slips, the
/// engine can stall, pressing it re-cranks). Sets `AssistProfile::auto_clutch`; logged as a
/// replay op.
#[wasm_bindgen]
pub fn set_clutch_mode(entity: u32, mode: &str) -> Result<(), JsValue> {
    let auto = match mode {
        "auto" => true,
        "manual" => false,
        _ => {
            return Err(err(format!(
                "clutch mode {mode:?}: want \"auto\" or \"manual\""
            )))
        }
    };
    with(|s| {
        let id = EntityId(entity);
        let mut p = s
            .assists(id)
            .ok_or_else(|| format!("unknown entity {entity}"))?;
        p.auto_clutch = auto;
        s.set_assists(id, p)
    })
}

/// Clutch mode of a vehicle: `"auto"` or `"manual"`.
#[wasm_bindgen]
pub fn clutch_mode(entity: u32) -> Result<String, JsValue> {
    with(|s| {
        s.assists(EntityId(entity))
            .map(|p| if p.auto_clutch { "auto" } else { "manual" }.to_string())
            .ok_or_else(|| format!("unknown entity {entity}"))
    })
}

/// Whether a vehicle's engine is stalled (manual clutch; press the clutch to re-crank).
#[wasm_bindgen]
pub fn engine_stalled(entity: u32) -> Result<bool, JsValue> {
    with(|s| {
        s.vehicle(EntityId(entity))
            .map(|v| v.vehicle.telemetry().stalled)
            .ok_or_else(|| format!("unknown entity {entity}"))
    })
}

/// Drift-assist tuning fields, in the order [`set_drift_tuning`] / [`drift_tuning`] use
/// (owner rows 90 / 91 / 98; see `wp_vehicle::DriftTuning`).
pub const DRIFT_TUNING_FIELDS: [&str; 8] = [
    "align_onset_rad",
    "align_full_rad",
    "slew_boost",
    "counter_lock_onset_rad",
    "counter_lock_full_rad",
    "hold_gain",
    "throttle_shaping",
    "envelope_deg",
];

/// Set a vehicle's drift-assist tuning (the testbed drift lab's live tuning panel): the 8
/// values of [`DRIFT_TUNING_FIELDS`] in order. A dev override only: it never touches the car
/// data, and it is not logged as a replay op (a tuned session does not replay bit-exactly).
#[wasm_bindgen]
pub fn set_drift_tuning(entity: u32, values: &[f32]) -> Result<(), JsValue> {
    if values.len() != DRIFT_TUNING_FIELDS.len() {
        return Err(err(format!(
            "drift tuning: want {} values ({:?}), got {}",
            DRIFT_TUNING_FIELDS.len(),
            DRIFT_TUNING_FIELDS,
            values.len()
        )));
    }
    let t = wp_vehicle::DriftTuning {
        align_onset_rad: values[0],
        align_full_rad: values[1],
        slew_boost: values[2],
        counter_lock_onset_rad: values[3],
        counter_lock_full_rad: values[4],
        hold_gain: values[5],
        throttle_shaping: values[6],
        envelope_deg: values[7],
    };
    with(|s| s.set_drift_tuning(EntityId(entity), t))
}

/// A vehicle's drift-assist tuning, the 8 values of [`DRIFT_TUNING_FIELDS`] in order.
#[wasm_bindgen]
pub fn drift_tuning(entity: u32) -> Result<Vec<f32>, JsValue> {
    with(|s| {
        let t = s
            .vehicle(EntityId(entity))
            .map(|v| v.vehicle.drift_tuning())
            .ok_or_else(|| format!("unknown entity {entity}"))?;
        Ok(vec![
            t.align_onset_rad,
            t.align_full_rad,
            t.slew_boost,
            t.counter_lock_onset_rad,
            t.counter_lock_full_rad,
            t.hold_gain,
            t.throttle_shaping,
            t.envelope_deg,
        ])
    })
}

/// Drift readout of a vehicle: `[body slip °, assist mode, envelope °]`; mode 0 off,
/// 1 straighten, 2 catch, 3 hold, 4 exceeded (`wp_vehicle::AssistMode::code`).
#[wasm_bindgen]
pub fn drift_state(entity: u32) -> Result<Vec<f32>, JsValue> {
    with(|s| {
        let t = s
            .vehicle(EntityId(entity))
            .map(|v| v.vehicle.telemetry())
            .ok_or_else(|| format!("unknown entity {entity}"))?;
        Ok(vec![
            t.body_slip_deg,
            t.assist_mode.code() as f32,
            t.envelope_deg,
        ])
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
    kind: String,
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

#[derive(Serialize)]
struct CarModelInfo {
    car_id: String,
    body_size_m: [f32; 3],
    wheels: Vec<WheelInfo>,
}

/// Static info JSON for a car model without a vehicle (`{ car_id, body_size_m, wheels }`, wheels
/// as in `vehicle_info`): the browser builds a model's instanced far-LOD mesh before any car
/// of that model exists.
#[wasm_bindgen]
pub fn car_model_info(car_id: &str) -> Result<String, JsValue> {
    let car = data::load_car(car_id).map_err(err)?;
    let d = &car.def;
    Ok(json(&CarModelInfo {
        car_id: d.id.clone(),
        body_size_m: d.body_size_m,
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
    }))
}

/// Static info JSON for a vehicle: scenario name, dimensions, wheels, CG (chassis space),
/// lattice sizes and the name tables for region / component / surface indices.
#[wasm_bindgen]
pub fn vehicle_info(entity: u32) -> Result<String, JsValue> {
    with(|s| {
        let kind = s
            .vehicle(EntityId(entity))
            .map(|v| v.kind)
            .ok_or_else(|| format!("unknown entity {entity}"))?;
        // Scenario entities have a name and spawn pose; sim-spawned ones (police, traffic) don't.
        let (name, spawn_pos) = match s.spec(EntityId(entity)) {
            Some(spec) => (spec.id.clone(), spec.pos),
            None => (
                format!("{}{entity}", kind_name(kind).to_lowercase()),
                [0.0; 3],
            ),
        };
        let car = car_of(s, entity)?;
        let d = &car.def;
        Ok(json(&VehicleInfo {
            entity,
            name,
            kind: kind_name(kind).into(),
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
            spawn_pos,
        }))
    })
}

fn kind_name(k: wp_sim::VehicleKind) -> &'static str {
    match k {
        wp_sim::VehicleKind::Player => "Player",
        wp_sim::VehicleKind::AiRacer => "AiRacer",
        wp_sim::VehicleKind::Police => "Police",
        wp_sim::VehicleKind::Traffic => "Traffic",
    }
}

/// Floats per vehicle in `vehicle_meta()`.
pub const META_STRIDE: usize = 3;

/// Per-vehicle render metadata, same order as `vehicle_states()`, 3 floats each:
/// `[entity, kind (0 player, 1 AI racer, 2 police, 3 physical traffic), flags]` with flags
/// 1 siren, 2 headlights, 4 braking, 8 horn (from the input the vehicle is driving with; a
/// police car's siren and light bar follow its unit's assignment, see [`police::siren_on`]).
#[wasm_bindgen]
pub fn vehicle_meta() -> Result<Vec<f32>, JsValue> {
    get(|s| {
        let mut out = Vec::with_capacity(s.sim().vehicles().len() * META_STRIDE);
        for v in s.sim().vehicles() {
            let kind = match v.kind {
                wp_sim::VehicleKind::Player => 0.0,
                wp_sim::VehicleKind::AiRacer => 1.0,
                wp_sim::VehicleKind::Police => 2.0,
                wp_sim::VehicleKind::Traffic => 3.0,
            };
            let i = &v.input;
            let flags = police::siren_on(s.sim(), v) as u32
                | (i.headlights as u32) << 1
                | ((i.brake > 0.1) as u32) << 2
                | (i.horn as u32) << 3;
            out.extend_from_slice(&[v.id.0 as f32, kind, flags as f32]);
        }
        out
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

// ----- weather -----------------------------------------------------------------------------

/// The weather selector's choices as JSON `[[id, label], …]` ([`weather::CHOICES`]).
#[wasm_bindgen]
pub fn weather_choices() -> String {
    json(&weather::CHOICES)
}

/// Scenario JSON with the weather of a selector choice (`"none"`, `"clear"`, `"overcast"`,
/// `"rain"`, `"heavy_rain"`, `"fog"`, `"dynamic"`); the session must be restarted with it
/// (weather is part of the scenario, so replays carry it).
#[wasm_bindgen]
pub fn scenario_with_weather(scenario_json: &str, choice: &str) -> Result<String, JsValue> {
    let sc: simcli::Scenario = serde_json::from_str(scenario_json).map_err(err)?;
    Ok(json(&weather::with_choice(sc, choice).map_err(err)?))
}

/// The selector choice matching a scenario's weather (`"none"`, a choice id, or `"custom"`).
#[wasm_bindgen]
pub fn scenario_weather_choice(scenario_json: &str) -> Result<String, JsValue> {
    let sc: simcli::Scenario = serde_json::from_str(scenario_json).map_err(err)?;
    Ok(weather::choice_of(&sc).to_string())
}

/// "Rainy night" city chase JSON: [`city_chase_city_json`] in a held downpour on a soaked road.
#[wasm_bindgen]
pub fn city_chase_rain_json(car: &str, mode: &str, city_id: &str) -> Result<String, JsValue> {
    let mode = data::parse_mode(mode).map_err(err)?;
    Ok(json(&weather::rainy_night(scenario::city_chase_in(
        car,
        mode,
        scenario::city_spec(city_id),
    ))))
}

/// Per-frame weather state ([`weather::info`], [`weather::INFO_STRIDE`] floats), local values at
/// the session's first entity (the player).
#[wasm_bindgen]
pub fn weather_info() -> Result<Vec<f32>, JsValue> {
    get(|s| {
        let focus = s.entities().first().and_then(|id| s.vehicle(*id));
        weather::info(s.sim(), focus)
    })
}

/// Wetness grid placement `[origin_x, origin_y, cell_m, nx, ny]` (empty without weather).
#[wasm_bindgen]
pub fn weather_grid_meta() -> Result<Vec<f32>, JsValue> {
    get(|s| weather::grid_meta(s.sim()))
}

/// Wetness grid, 2 bytes per cell (wetness × 255, standing water mm × 40); see
/// [`weather::grid_bytes`]. Call at about 1 Hz.
#[wasm_bindgen]
pub fn weather_grid() -> Result<Vec<u8>, JsValue> {
    get(|s| weather::grid_bytes(s.sim()))
}

/// Spray intensity 0..1 per vehicle, same order as `vehicle_states()` ([`weather::spray`]).
#[wasm_bindgen]
pub fn weather_spray() -> Result<Vec<f32>, JsValue> {
    get(|s| weather::spray(s.sim()))
}

/// `Sim::state_hash()` as 16 hex digits.
#[wasm_bindgen]
pub fn state_hash() -> Result<String, JsValue> {
    get(|s| format!("{:016x}", s.sim().state_hash()))
}

/// Wanted level / heat of a racer as JSON ([`police::PoliceStatus`]): level, heat, heat
/// within the level, pursuit status, escape progress, units in pursuit, bounty. `null` when the
/// police system is not stepped in this session.
#[wasm_bindgen]
pub fn police_status(entity: u32) -> Result<String, JsValue> {
    get(|s| {
        if !s.sim().systems().police {
            return "null".to_string();
        }
        json(&police::status(s.sim().police(), EntityId(entity)))
    })
}

/// Police units that are not sim vehicles (internally driven cars, helicopters), 6 floats
/// each: `[x, y, z, yaw, speed_mps, flags]` (1 helicopter, 2 pursuing). Bound units are sim
/// vehicles of kind Police and come through `vehicle_states()` / `vehicle_meta()`.
#[wasm_bindgen]
pub fn police_units() -> Result<Vec<f32>, JsValue> {
    get(|s| {
        let world = s.sim().world();
        police::units(s.sim().police(), &|p| world.map().ground_height(p, None))
    })
}

/// Static map of the session's city as JSON ([`map::MapStatic`]): road centrelines by class
/// and layer (grade / bridge deck), district polygons, POIs, signalised junctions. `null` on
/// the flat world. Fetch once per city (UI kit minimap and full map).
#[wasm_bindgen]
pub fn map_static_json() -> Result<String, JsValue> {
    get(|s| {
        if !s.has_city() {
            return "null".to_string();
        }
        json(&map::map_static(s.sim().world().map()))
    })
}

/// Per-frame map data for `player` ([`map::map_dynamic`]): a header (status, level, search
/// zone, escape progress, sight ranges, player pose), then every police unit
/// (`map::UNIT_STRIDE` floats) and every roadblock / spike strip (`map::TOOL_STRIDE`).
/// Without police in the session the header only carries the player pose.
#[wasm_bindgen]
pub fn map_dynamic(player: u32) -> Result<Vec<f32>, JsValue> {
    get(|s| {
        let pose = s
            .sim()
            .vehicle_states()
            .into_iter()
            .find(|(id, _)| id.0 == player)
            .map(|(_, st)| map::pose_of(&st));
        let police = s.sim().systems().police.then(|| s.sim().police());
        map::map_dynamic(police, EntityId(player), pose)
    })
}

/// End-of-run result as JSON ([`run::RunReport`]): outcome (`Escaped`, `Safehouse`,
/// `Captured`, `CapturedTotalLoss`, `TotalLoss`), bounty payout, fines, bail, impound decision
/// and fee, G's repair quote, cash before and after, and this settlement's ledger lines.
/// `null` until the player's first chase (or car) ends, or when the session has no economy run
/// (replays, scripted scenarios, no police). `seq` grows with each settlement.
#[wasm_bindgen]
pub fn run_result_json() -> Result<String, JsValue> {
    get(|s| json(&s.run().and_then(|r| r.report())))
}

/// Every ledger entry of the player's account as JSON ([`run::LedgerLine`] array, oldest first;
/// carries over restarts). `[]` without an economy run.
#[wasm_bindgen]
pub fn run_ledger_json() -> Result<String, JsValue> {
    get(|s| {
        let lines: Vec<run::LedgerLine> = s
            .run()
            .map(|r| r.ledger().iter().map(run::LedgerLine::of).collect())
            .unwrap_or_default();
        json(&lines)
    })
}

/// `{ cash, rep, bounty, debt }` of the player's account, or `null` without an economy run.
#[wasm_bindgen]
pub fn run_account_json() -> Result<String, JsValue> {
    get(|s| {
        json(&s.run().map(|r| {
            let a = r.account();
            serde_json::json!({ "cash": a.cash, "rep": a.rep, "bounty": a.bounty, "debt": a.debt() })
        }))
    })
}

/// "Repair & continue" after a capture or wreck: pays the impound fee and G's `RepairAll` from
/// the ledger, repairs the car in place (replay op `Repair`) and releases the brakes. Returns
/// [`run::ContinueReport`] JSON; an error (nothing changed) if the run is not over, the car is
/// gone, or the player cannot afford it.
#[wasm_bindgen]
pub fn run_repair_continue() -> Result<String, JsValue> {
    with(|s| s.repair_and_continue().map(|r| json(&r)))
}

// ----- races (workstream 15) ----------------------------------------------------------------

fn parse_mode(mode: &str) -> Result<DamageMode, String> {
    match mode {
        "Simulation" => Ok(DamageMode::Simulation),
        "Arcade" | "" => Ok(DamageMode::Arcade),
        other => Err(format!("unknown damage mode {other:?}")),
    }
}

/// The race picker as JSON ([`race::RaceCard`] array): every race with its city, kind, laps,
/// legality, hour, length, police presence, AI drivers (name, personality, car) and the
/// advertised purse / buy-in for a player in `car` in `mode`.
#[wasm_bindgen]
pub fn race_list(car: &str, mode: &str) -> Result<String, JsValue> {
    let mode = parse_mode(mode).map_err(err)?;
    let car = simcli::data::load_car(data::mem_data(), car).ok();
    Ok(json(&race::race_cards(car.as_ref(), mode)))
}

/// Start race `id` with the player in `car` (`mode` `"Arcade"` | `"Simulation"`): a new live
/// session on the race's grid, countdown running, AI field on the slots ahead. `autopilot`
/// hands the player's car to the AI (tests / e2e). The account carries over like a restart.
/// Returns the entity ids (player first).
#[wasm_bindgen]
pub fn race_start(id: &str, car: &str, mode: &str, autopilot: bool) -> Result<Vec<u32>, JsValue> {
    let def = wp_race::catalog::race(id).ok_or_else(|| err(format!("unknown race {id:?}")))?;
    let sc =
        race::race_scenario(&def, car, parse_mode(mode).map_err(err)?, autopilot).map_err(err)?;
    Ok(install(Session::new(sc, false).map_err(err)?))
}

/// Live race state, or an empty array without a race: see [`race::RaceRun::info`]
/// (`[phase, countdown_s, elapsed_s, laps, gates, player, racers, closed_loop, length_m,
/// autopilot, kind, player_done]`).
#[wasm_bindgen]
pub fn race_info() -> Result<Vec<f32>, JsValue> {
    get(|s| s.race().map(|r| r.info(s.tick())).unwrap_or_default())
}

/// Live standings, position order, [`race::STANDING_STRIDE`] floats per racer (see
/// [`race::RaceRun::standings_packed`]); empty without a race.
#[wasm_bindgen]
pub fn race_standings() -> Result<Vec<f32>, JsValue> {
    get(|s| {
        s.race()
            .map(race::RaceRun::standings_packed)
            .unwrap_or_default()
    })
}

/// Floats per racer in `race_standings()`.
#[wasm_bindgen]
pub fn race_standing_stride() -> u32 {
    race::STANDING_STRIDE as u32
}

/// Gates and route of the current race (static; see [`race::RaceRun::course_packed`]).
#[wasm_bindgen]
pub fn race_course() -> Result<Vec<f32>, JsValue> {
    get(|s| {
        s.race()
            .map(race::RaceRun::course_packed)
            .unwrap_or_default()
    })
}

/// Results panel JSON ([`race::RaceReport`]) once the player is no longer racing (finished,
/// DNF or DQ), `null` before or without a race. `final_results` turns true when the whole
/// field is done; `payout` (G's settlement) arrives then. `handoff_run_end`: the police caught
/// the player, the end-of-run panel owns the screen.
#[wasm_bindgen]
pub fn race_result_json() -> Result<String, JsValue> {
    get(|s| json(&s.race_report()))
}

/// Debug / e2e: set a racer's police heat (`PoliceSystem::set_heat`). Changes the run and is
/// **not** recorded in replays.
#[wasm_bindgen]
pub fn debug_set_heat(entity: u32, heat: f32) -> Result<(), JsValue> {
    with(|s| {
        s.debug_sim_mut()?
            .police_mut()
            .set_heat(EntityId(entity), heat);
        Ok(())
    })
}

/// Debug / e2e: park `entity` (stopped) a few metres in front of the first police car.
/// Returns the police car's entity id. Not recorded in replays.
#[wasm_bindgen]
pub fn debug_park_near_police(entity: u32, ahead_m: f32) -> Result<u32, JsValue> {
    with(|s| {
        s.debug_park_near_police(EntityId(entity), ahead_m)
            .map(|e| e.0)
    })
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

/// Floats per record in `traffic_draw()` / `police_draw()` ([`lod::DRAW_STRIDE`]).
#[wasm_bindgen]
pub fn lod_draw_stride() -> u32 {
    lod::DRAW_STRIDE as u32
}

/// Draw records for every civilian agent at any LOD (see [`lod::traffic_draw`]), 10 floats each:
/// `[agent id, entity (0 = none), x, y, z, qx, qy, qz, qw, flags]`; flags as in
/// `traffic_agents()`. Kinematic agents are seated on the ground the way the sim places a
/// promoted car, so the browser can swap instanced and full meshes without a jump.
#[wasm_bindgen]
pub fn traffic_draw() -> Result<Vec<f32>, JsValue> {
    get(|s| lod::traffic_draw(s.sim()))
}

/// Draw records for every police unit, bound or not (see [`lod::police_draw`]), 10 floats each:
/// `[unit id, entity (0 = none), x, y, z, qx, qy, qz, qw, flags]`, flags 1 helicopter,
/// 2 pursuing.
#[wasm_bindgen]
pub fn police_draw() -> Result<Vec<f32>, JsValue> {
    get(|s| lod::police_draw(s.sim()))
}

/// Car models the current session can show, as JSON `{ cars, traffic, police }`
/// ([`lod::SessionModels`]), for the browser to preload their meshes before they appear.
#[wasm_bindgen]
pub fn session_car_models() -> Result<String, JsValue> {
    get(|s| json(&lod::session_car_models(s.scenario(), s.has_city())))
}

/// Floats per head in `signal_heads()`.
#[wasm_bindgen]
pub fn signal_head_stride() -> u32 {
    signals::HEAD_STRIDE as u32
}

/// Static traffic signal heads of the session's city (empty without one), one per signalised
/// junction approach, 10 floats each:
/// `[x, y, z, yaw, left_m, right_m, radius_m, centre_z, intersection, group]`
/// (see [`signals::heads`]). Fetch once per session.
#[wasm_bindgen]
pub fn signal_heads() -> Result<Vec<f32>, JsValue> {
    get(|s| signals::heads(s.sim().world()))
}

/// Current state per head in `signal_heads()` order, one byte each: 0 off, 1 red, 2 yellow,
/// 3 green. Cheap enough to call every frame.
#[wasm_bindgen]
pub fn signal_states() -> Result<Vec<u8>, JsValue> {
    get(|s| signals::states(s.sim().world()))
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

/// Floats per panel in `panel_states()`.
pub const PANEL_STRIDE: usize = 10;

/// Hinged/bolted body panels of a vehicle (workstream R; empty unless the scenario has
/// `panels: true`), in `data/panels/<car>.json` order, [`PANEL_STRIDE`] floats each:
/// `[region index (like vehicle_info().regions), state (0 Attached, 1 Unlatched,
/// 2 HangingByOne, 3 Detached), angle rad, angular velocity rad/s, pivot x, y, z, axis x, y, z]`
/// (chassis space). A free panel's lattice nodes are already rotated; renderers use pivot,
/// axis and angle to swing the rigid panel mesh about its hinge.
#[wasm_bindgen]
pub fn panel_states(entity: u32) -> Result<Vec<f32>, JsValue> {
    with(|s| {
        let v = s
            .sim()
            .vehicles()
            .iter()
            .find(|v| v.id == EntityId(entity))
            .ok_or_else(|| format!("unknown entity {entity}"))?;
        let panels = v.damage.panel_states();
        let mut out = Vec::with_capacity(panels.len() * PANEL_STRIDE);
        for p in panels {
            out.push(pack::region_index(p.region) as f32);
            out.push(p.state.code() as f32);
            out.push(p.angle_rad);
            out.push(p.omega_rad_s);
            out.extend_from_slice(&p.pivot);
            out.extend_from_slice(&p.axis);
        }
        Ok(out)
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
