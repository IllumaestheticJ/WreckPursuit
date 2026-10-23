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
//! | session | `init(seed, scenario_json, scripted)`, `free_drive_json(car, mode, city)`, `free_drive_city_json(car, mode, city_id)`, `city_chase_city_json(car, mode, city_id)`, `city_id()`, `scenario_from_ron(text)`, `scenario_with_speed(json, mps)`, `step(dt) -> alpha`, `set_damage_mode(str)`, `car_ids()`, `car_stats_json()`, `car_stats_build_json(car, mods, mode)` |
//! | control | `spawn_vehicle(car_id, pos, yaw)`, `set_input(entity, f32[])`, `set_assist_profile(entity, json)`, `assist_preset(source)`, `set_countersteer(entity, %)` / `countersteer(entity)`, `set_clutch_mode(entity, "auto"|"manual")` / `clutch_mode(entity)`, `engine_stalled(entity)`, `assist_presets()`, `assist_preset_named(name, source)`, `assist_preset_of(entity, source)`, `reset_vehicle(entity)` |
//! | state | `entities()`, `vehicle_states()`, `vehicle_states_prev()`, `vehicle_info(entity)`, `world_info()`, `state_hash()`, `traffic_agents()`, `traffic_models()`, `police_models()`, `signal_heads()`, `signal_states()`, `road_markings(yellow)` |
//! | damage | `lattice_nodes`, `lattice_rest`, `lattice_beams`, `lattice_node_regions`, `lattice_strain`, `region_energy`, `skin_binding_indices`, `skin_binding_weights` |
//! | io | `events_drain() -> postcard`, `events_decode(bytes)`, `telemetry(entity)`, `telemetry_names()`, `ffb(entity)`, `intents()` |
//! | replay | `record_start()`, `record_stop()`, `recording()`, `replay_load(bytes)`, `replay_seek(tick)`, `replay_play(speed)`, `replay_status()`, `replay_export_csv(bytes, entity, every)` |
//! | scenario | `scenario_json()`, `scenario_result()` |
//! | crash replay (lead) | `crash_take()`, `crash_tuning_json()`, `crash_tuning_set(json)`, `crash_detect_enable(on)`, `crash_buffer_ticks()`, `crash_replay_start(seq)`, `crash_replay_active()`, `crash_replay_stop()` — see [`crash_replay`] |
//! | touge | `touge_list_json()`, `touge_menu_json(map, dir, daylight, surface, opponent)`, `touge_scenario_json(…, car, mode, wager_json, autopilot)`, `touge_start(…)`, `touge_result_json()`, `touge_debug_set_rep(rep)` — see [`touge`] |
//! | race (15) | `race_list(car, mode)`, `race_start(id, car, mode, autopilot)`, `race_bet(label, stake)`, `race_bet_status()`, `roster_json()`, `roster_state()` / `roster_state_load(text)`, `race_info()`, `race_standings()`, `race_course()`, `race_result_json()` — see [`race`] |
//! | run (lead) | `run_result_json()`, `run_ledger_json()`, `run_account_json()`, `run_repair_continue()`, `debug_set_heat(entity, heat)`, `debug_park_near_police(entity, ahead_m)` — see [`run`] |
//! | props (lead) | `props_static()` once per session, `props_moved()` per frame, `prop_stride()`, `prop_moved_stride()`, `prop_pose(i)`, `debug_knock_prop(entity, prop, speed)` — see [`props`] |
//! | parked-car damage (lead) | `parked_wrecks()` per frame, `parked_wreck_stride()`, `parked_entity_base()`, `parked_wreck_shattered(entity)`; a wreck's entity works with the `lattice_*` / `vehicle_info` calls — see [`props`] |
//! | map (16) | `map_static_json()` once per city, `map_dynamic(player)` per frame — see [`map`] |
//! | weather (lead) | `weather_choices()`, `scenario_with_weather(json, choice)`, `scenario_weather_choice(json)`, `city_chase_rain_json(car, mode, city_id)`, `weather_info()`, `weather_grid_meta()`, `weather_grid()`, `weather_spray()` — see [`weather`] |
//! | career (G) | `career_new(mode)`, `career_load_json`, `career_save_json`, `career_view_json`, `career_mods_json(uid)`, `career_start_run(scenario_json)`, `career_end_run`, `career_run_active`, `career_buy_car`, `career_floor_car`, `career_sell_car`, `career_set_active`, `career_install_mod`, `career_uninstall_mod`, `career_quote_json`, `career_repair`, `career_release`, `career_insure`, `career_next_night` — see [`career`] |
//! | drift lab B (lead) | `drift_tuning_json()`, `drift_tuning_defaults_json()`, `drift_tuning_set(json)`, `drift_readout(entity)` — see [`drift_lab`] |
//! | mods | `mod_catalog_json(car)`, `build_visuals_json(car, mods)`, `entity_visuals_json(entity)` — see [`mod_visuals`]; install with `EntitySpec::mods` in the scenario |
//! | run (lead) | `run_result_json()`, `run_ledger_json()`, `run_account_json()`, `run_repair_continue()`, `debug_set_heat(entity, heat)`, `debug_park_near_police(entity, ahead_m)`, `debug_place(entity, x, y, yaw, speed)` — see [`run`] |
//! | garages (lead) | `garages_json()`, `garage_visit_json()`, `garage_repair()`, `garage_leave()` — see [`garage_visit`] |
//! | audio (13) | `audio_library()`, `audio_frame(listener, events_json, focus)`, `audio_set_siren`, `audio_set_scanner_tier`, `audio_radio`, `audio_reset()` — see [`audio`] |

#![warn(missing_docs)]

/// Multiplayer client surface (`net_*` functions, workstream F), see the module docs.
pub mod net;

pub mod audio;
pub mod build_stats;
pub mod career;
pub mod crash_replay;
pub mod data;
pub mod drift_lab;
pub mod garage;
pub mod garage_visit;
pub mod lod;
pub mod map;
pub mod mod_visuals;
pub mod pack;
pub mod police;
pub mod props;
pub mod race;
pub mod reset;
pub mod roam;
pub mod run;
pub mod scenario;
pub mod session;
pub mod signals;
pub mod siren_cue;
pub mod street;
pub mod touge;
pub mod weather;

#[cfg(test)]
mod career_tests;
#[cfg(test)]
mod collide_tests;
#[cfg(test)]
mod crash_replay_tests;
#[cfg(test)]
mod police_audio_trace;
#[cfg(test)]
mod race_tests;
#[cfg(test)]
mod roam_tests;
#[cfg(test)]
mod run_tests;
#[cfg(test)]
mod street_tests;
#[cfg(test)]
mod touge_tests;

use std::cell::RefCell;

use serde::Serialize;
use wasm_bindgen::prelude::*;
use wp_contracts::{AssistProfile, DamageMode, EntityId, InputSource, SimEvent, PHYSICS_DT};

use crate::session::Session;

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    /// What the browser was last told about the session's props (`props_moved`).
    static PROP_SYNC: RefCell<props::PropSync> = RefCell::new(props::PropSync::default());
    /// The live session while a crash replay plays in `SESSION` (see [`crash_replay`]).
    static PARKED: RefCell<Option<Session>> = const { RefCell::new(None) };
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

/// Run `f` with the current *live* session, if any (career glue: works with no session too).
/// During a crash replay that is the parked live session, not the replay.
fn with_session_opt<T, E>(f: impl FnOnce(Option<&mut Session>) -> Result<T, E>) -> Result<T, E> {
    PARKED.with(|p| match p.borrow_mut().as_mut() {
        Some(live) => f(Some(live)),
        None => SESSION.with(|s| f(s.borrow_mut().as_mut())),
    })
}

fn get<T>(f: impl FnOnce(&mut Session) -> T) -> Result<T, JsValue> {
    with(|s| Ok(f(s)))
}

fn install(mut session: Session) -> Vec<u32> {
    let ids = session.entities().iter().map(|e| e.0).collect();
    SESSION.with(|s| {
        let mut slot = s.borrow_mut();
        // A crash replay being replaced: the live session it parked is the one that ends.
        if let Some(live) = PARKED.with(|p| p.borrow_mut().take()) {
            *slot = Some(live);
        }
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
        PROP_SYNC.with(|p| p.borrow_mut().reset(session.sim()));
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

/// Free roam in a chosen city (`city_id` as in [`free_drive_city_json`]): free drive plus
/// `roamers` AI street racers (0 = the default 10, at most 24) roaming the city's highways,
/// touge, dirt roads, downtown and countryside. Start it with `init` like any scenario.
#[wasm_bindgen]
pub fn free_roam_city_json(
    car: &str,
    mode: &str,
    city_id: &str,
    roamers: u32,
) -> Result<String, JsValue> {
    let mode = data::parse_mode(mode).map_err(err)?;
    let n = match roamers {
        0 => roam::DEFAULT_ROAMERS,
        n => n as usize,
    };
    Ok(json(
        &roam::free_roam_in(car, mode, city_id, n).map_err(err)?,
    ))
}

/// Free-roam status JSON (`null` outside free roam): every roamer's entity id, home haunt,
/// phase, current leg's haunt, full-pace run or not, legs and distance driven; rescues so
/// far; the last few roamer events.
#[wasm_bindgen]
pub fn roam_status_json() -> Result<String, JsValue> {
    get(|s| json(&s.roam().map(roam::RoamRun::status)))
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

/// Menu stat bars for every embedded car as JSON (`simcli::car_stats::CarStatsTable`:
/// `{scales, cars: {id: {display_name, zero_to_100_s, top_speed_kmh, cornering_g,
/// bars: {acceleration, top_speed, cornering}}}}`, bars 0–1). Needs no session.
#[wasm_bindgen]
pub fn car_stats_json() -> Result<String, JsValue> {
    data::car_stats().map(json).map_err(err)
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

/// Names of the one-click assist presets, in menu order: `["Rookie","Street","Sim"]`.
#[wasm_bindgen]
pub fn assist_presets() -> String {
    let names: Vec<&str> = wp_vehicle::AssistPreset::ALL
        .iter()
        .map(|p| p.name())
        .collect();
    json(&names)
}

/// One-click preset `name` for a driver on `source`: JSON `{"mode": "Arcade"|"Simulation",
/// "assists": AssistProfile}`. Apply both (`set_damage_mode` + `set_assist_profile`).
#[wasm_bindgen]
pub fn assist_preset_named(name: &str, source: &str) -> Result<String, JsValue> {
    let preset = wp_vehicle::AssistPreset::from_name(name)
        .ok_or_else(|| err(format!("unknown preset {name}")))?;
    let source: InputSource =
        serde_json::from_value(serde_json::Value::String(source.into())).map_err(err)?;
    #[derive(Serialize)]
    struct PresetJson {
        mode: DamageMode,
        assists: AssistProfile,
    }
    Ok(json(&PresetJson {
        mode: preset.damage_mode(),
        assists: preset.assists(source),
    }))
}

/// The preset a vehicle's current assists and the session's damage mode match for `source`
/// (`"Rookie"` | `"Street"` | `"Sim"`), or `""` when customised.
#[wasm_bindgen]
pub fn assist_preset_of(entity: u32, source: &str) -> Result<String, JsValue> {
    let source: InputSource =
        serde_json::from_value(serde_json::Value::String(source.into())).map_err(err)?;
    get(|s| {
        s.assists(EntityId(entity))
            .and_then(|a| wp_vehicle::AssistPreset::matching(&a, s.mode(), source))
            .map(|p| p.name().to_string())
            .unwrap_or_default()
    })
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

/// Return a vehicle to its spawn pose, pristine (logged as a replay op).
#[wasm_bindgen]
pub fn reset_vehicle(entity: u32) -> Result<(), JsValue> {
    with(|s| s.reset_vehicle(EntityId(entity)))
}

/// Reset in place (testbed R, playtest row 65): the car goes back on its wheels at rest on the
/// nearest clear lane within 15 m, heading its own way, and is fully repaired — except in a
/// career run, where repairs are bought in the garage. Heat, pursuit and race progress carry
/// on. The car is a ghost for 1 s (passes through other cars, props and debris). Logged as
/// replay ops. Returns `{ x, y, yaw, on_lane, repaired, ghost_ticks }` as JSON.
#[wasm_bindgen]
pub fn reset_in_place(entity: u32) -> Result<String, JsValue> {
    let repair = !career::career_run_active();
    with(|s| s.reset_in_place(EntityId(entity), repair)).map(|p| {
        json(&serde_json::json!({
            "x": p.xy[0], "y": p.xy[1], "yaw": p.yaw, "on_lane": p.on_lane, "repaired": repair,
            "ghost_ticks": reset::GHOST_TICKS,
        }))
    })
}

/// Physics ticks a vehicle stays a ghost after a reset in place (0 = solid; see
/// `wp_sim::Sim::set_ghost`). The testbed draws ghost cars translucent.
#[wasm_bindgen]
pub fn ghost_ticks(entity: u32) -> Result<u32, JsValue> {
    get(|s| s.sim().ghost_ticks(EntityId(entity)).unwrap_or(0))
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
    /// Strut compression at rest (the car data's hub position is the wheel centre there), m:
    /// the renderer draws the wheel at `position_m.z + compression − static_compression_m`.
    static_compression_m: f32,
    tire_id: String,
}

/// Static strut compression of wheel `i` at the car's rest load (as `wp_vehicle` computes it).
fn static_compression_of(d: &wp_contracts::VehicleDef, i: usize) -> f32 {
    let axle_frac = if i < 2 {
        d.front_weight_frac
    } else {
        1.0 - d.front_weight_frac
    };
    let load = d.mass_kg * wp_contracts::GRAVITY_MPS2 * axle_frac * 0.5;
    wp_vehicle::suspension::static_compression(&d.suspension[i], load)
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
    /// `wp_police` unit id of a sim police vehicle (the `unit_id` of `police_units()`), so a
    /// unit keeps its number when it becomes a sim vehicle; `None` for other vehicles.
    police_unit: Option<u32>,
    /// `wp_world` agent id of promoted traffic (the `agent_id` of `traffic_agents()`), so the
    /// physical car can be painted like its instanced twin; `None` for other vehicles.
    traffic_agent: Option<u32>,
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
            .enumerate()
            .map(|(i, (w, su))| WheelInfo {
                position_m: w.position_m,
                radius_m: w.radius_m,
                width_m: w.width_m,
                steerable: w.steerable,
                travel_m: su.travel_m,
                static_compression_m: static_compression_of(d, i),
                tire_id: w.tire_id.clone(),
            })
            .collect(),
    }))
}

/// Static info JSON for a vehicle: scenario name, dimensions, wheels, CG (chassis space),
/// lattice sizes, the name tables for region / component / surface indices and, for sim police
/// vehicles, their `wp_police` unit id (`police_unit`); for promoted traffic, its `wp_world`
/// agent id (`traffic_agent`).
#[wasm_bindgen]
pub fn vehicle_info(entity: u32) -> Result<String, JsValue> {
    with(|s| {
        // A dented parked car (`parked_wrecks()`): kind "Parked", named after its prop.
        let parked = s.parked_wreck(EntityId(entity)).map(|w| w.prop);
        let kind = match parked {
            Some(_) => "Parked",
            None => s
                .vehicle(EntityId(entity))
                .map(|v| kind_name(v.kind))
                .ok_or_else(|| format!("unknown entity {entity}"))?,
        };
        // Scenario entities have a name and spawn pose; sim-spawned ones (police, traffic) don't.
        let (name, spawn_pos) = match (s.spec(EntityId(entity)), parked) {
            (_, Some(prop)) => (format!("parked{prop}"), [0.0; 3]),
            (Some(spec), None) => (spec.id.clone(), spec.pos),
            (None, None) => (format!("{}{entity}", kind.to_lowercase()), [0.0; 3]),
        };
        let car = car_of(s, entity)?;
        let d = &car.def;
        Ok(json(&VehicleInfo {
            entity,
            name,
            kind: kind.into(),
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
                .enumerate()
                .map(|(i, (w, su))| WheelInfo {
                    position_m: w.position_m,
                    radius_m: w.radius_m,
                    width_m: w.width_m,
                    steerable: w.steerable,
                    travel_m: su.travel_m,
                    static_compression_m: static_compression_of(d, i),
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
            police_unit: s
                .vehicle(EntityId(entity))
                .and_then(|v| v.police_unit)
                .map(|u| u.0),
            traffic_agent: s.vehicle(EntityId(entity)).and_then(|v| v.traffic_agent),
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
/// police car's siren and light bar follow its unit's assignment, see [`police::siren_on`]),
/// 16 left turn signal, 32 right turn signal (police cars: [`police::turn_signal`]; traffic
/// takes its lamps from `traffic_draw()`; players have no signal input yet);
/// plus, for physical traffic, its agent's colour `variant << 8` (same as in
/// `traffic_agents()`, so a car keeps its colour when promoted).
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
            let variant = v.traffic_agent.map_or(0, |id| {
                s.sim()
                    .world()
                    .agents()
                    .find(|a| a.id == id)
                    .map_or(0, |a| a.variant as u32)
            });
            let flags = police::siren_on(s.sim(), v) as u32
                | (i.headlights as u32) << 1
                | ((i.brake > 0.1) as u32) << 2
                | (i.horn as u32) << 3;
            let signal = police::turn_signal(s.sim(), v);
            let flags =
                flags | ((signal < 0) as u32) << 4 | ((signal > 0) as u32) << 5 | variant << 8;
            out.extend_from_slice(&[v.id.0 as f32, kind, flags as f32]);
        }
        out
    })
}

/// `[tick, time_of_day_h, night_factor, mode (0 Arcade / 1 Simulation / 2 Extreme), replay (0/1),
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
            match s.mode() {
                DamageMode::Arcade => 0.0,
                DamageMode::Simulation => 1.0,
                DamageMode::Extreme => 2.0,
            },
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

/// Police units that are not sim vehicles (internally driven cars, helicopters), 8 floats
/// each: `[x, y, z, yaw, speed_mps, flags, unit_id, model]` (flags 1 helicopter, 2 pursuing;
/// `unit_id` is the stable `wp_police` unit id; `model` indexes `police_models()`, -1 for
/// helicopters, single-model sessions and units not yet reconciled). A unit keeps its model
/// when it later becomes a sim vehicle of kind Police (`vehicle_states()` / `vehicle_meta()`,
/// car id via `vehicle_info()`).
#[wasm_bindgen]
pub fn police_units() -> Result<Vec<f32>, JsValue> {
    get(|s| {
        let sim = s.sim();
        let world = sim.world();
        police::units(
            sim.police(),
            &|p| world.map().ground_height(p, None),
            &|u| sim.police_unit_model(u).map_or(-1.0, |m| m as f32),
        )
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
/// zone, escape progress, sight ranges, player pose, racer count), then every police unit
/// (`map::UNIT_STRIDE` floats), every roadblock / spike strip (`map::TOOL_STRIDE`) and,
/// during a race, every racer in position order (`map::RACER_STRIDE`). Without police in
/// the session there are no units or tools.
#[wasm_bindgen]
pub fn map_dynamic(player: u32) -> Result<Vec<f32>, JsValue> {
    get(|s| map::map_dynamic_for(s, EntityId(player)))
}

/// Helicopters tracking a racer, 8 floats each: `[x, y, z, aim_x, aim_y, aim_z,
/// cone_half_angle_rad, flags]` (1 = has visual). For the spotlight; the airframe itself comes
/// through `police_draw()` (flag 1).
#[wasm_bindgen]
pub fn police_helicopters() -> Result<Vec<f32>, JsValue> {
    get(|s| {
        let world = s.sim().world();
        police::helicopters(s.sim().police(), &|p, z| world.map().ground_height(p, z))
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
        "Extreme" => Ok(DamageMode::Extreme),
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
    race_start_tuned(id, car, mode, autopilot, "")
}

/// [`race_start`] with the player's free car tune (`wp_vehicle::CarTune` JSON, e.g.
/// `{"suspension":5,"gearing":4}`; empty = stock) on the player entity.
#[wasm_bindgen]
pub fn race_start_tuned(
    id: &str,
    car: &str,
    mode: &str,
    autopilot: bool,
    tune_json: &str,
) -> Result<Vec<u32>, JsValue> {
    let def = wp_race::catalog::race(id).ok_or_else(|| err(format!("unknown race {id:?}")))?;
    let mut sc =
        race::race_scenario(&def, car, parse_mode(mode).map_err(err)?, autopilot).map_err(err)?;
    if !tune_json.is_empty() {
        let tune: wp_vehicle::CarTune =
            serde_json::from_str(tune_json).map_err(|e| err(format!("bad tune: {e}")))?;
        if let Some(player) = sc.entities.first_mut() {
            player.tune = tune;
        }
    }
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

// ----- street racers (src/street.rs) -------------------------------------------------------

/// Add the NPC street racers (8 by day, 13 at night, [`street::ROSTER`]) to live free-drive /
/// city-chase city sessions started after this call (default on). They are scenario entities (`street_<level>_<name>`, after the
/// player in `entities()`), so recordings and replays carry them.
#[wasm_bindgen]
pub fn set_street_racers(on: bool) {
    session::set_street_racers_enabled(on);
}

/// Street racers for the map, 6 floats each: `[entity, level, x, y, yaw, mode]` (`level` as in
/// [`street::LEVELS`]: 2..=5, there is no level 1; mode
/// 0 roaming, 1 waiting on a challenge, 2 racing the player). Off-shift racers (in the lockup)
/// are left out. Empty without street racers.
#[wasm_bindgen]
pub fn street_racers() -> Result<Vec<f32>, JsValue> {
    get(|s| s.street().map(|st| st.packed(s.sim())).unwrap_or_default())
}

/// Street-race state as JSON: `{ levels, roster, hint, pursued, blocked, offer, race, result }`
/// — the level table ([`street::LEVELS`]), every racer ([`street::RosterEntry`]), the racer a
/// honk would challenge now (`hint`), whether the police are after the player (no challenges
/// then) and the racer in reach they block (`blocked`),
/// the open challenge ([`street::Offer`]), the race in progress
/// ([`street::RaceView`]) and the latest result ([`street::RaceResult`], `seq` grows). `null`
/// without street racers.
#[wasm_bindgen]
pub fn street_status_json() -> Result<String, JsValue> {
    get(|s| {
        let Some(st) = s.street() else {
            return "null".to_string();
        };
        let player = s.run().map(|r| r.player());
        json(&serde_json::json!({
            "levels": street::LEVELS,
            "roster": st.roster(),
            "hint": player.and_then(|p| st.challengeable(s.sim(), p)),
            // Wanted or chased: no challenges (owner row 150); `blocked` = the racer a honk
            // would challenge without the police.
            "pursued": player.is_some_and(|p| s.sim().police().is_wanted_or_pursued(p)),
            "blocked": player
                .filter(|p| s.sim().police().is_wanted_or_pursued(*p))
                .and_then(|p| st.in_reach(s.sim(), p)),
            "offer": st.offer(s.sim(), s.run()),
            "race": st.race(s.sim()),
            "result": st.result(),
        }))
    })
}

/// Waypoints from a flat `[x0, y0, x1, y1, …]` array.
fn waypoints(via: &[f32]) -> Vec<[f32; 2]> {
    via.chunks_exact(2).map(|c| [c[0], c[1]]).collect()
}

/// Plan the open challenge's route from the player through the waypoints `via` (flat
/// `[x0, y0, x1, y1, …]`, in order; may be empty) to `(x, y)` ([`street::Preview`] JSON:
/// length, snapped finish and gates, centreline, whether it is raceable).
#[wasm_bindgen]
pub fn street_preview(x: f32, y: f32, via: Vec<f32>) -> Result<String, JsValue> {
    with(|s| s.street_preview([x, y], &waypoints(&via)).map(|p| json(&p)))
}

/// Accept the open challenge: race through the waypoints `via` (as in [`street_preview`]) to
/// `(x, y)` for `stake` dollars (paid now; a win pays twice the stake). Both cars are reset
/// side by side at the start. Errors (nothing changed) on a bad stake, no route, no open
/// challenge, or while the police are after the player.
#[wasm_bindgen]
pub fn street_accept(x: f32, y: f32, via: Vec<f32>, stake: f64) -> Result<(), JsValue> {
    with(|s| s.street_accept([x, y], &waypoints(&via), stake.round() as i64))
}

/// The street race's gates (waypoints, then B) and route, as [`race_course`] lays them out:
/// `[gates, (x, y, z, dx, dy, width) × gates, (x, y, z) × points]`. Empty without a street race.
#[wasm_bindgen]
pub fn street_course() -> Result<Vec<f32>, JsValue> {
    get(|s| {
        s.street()
            .map(|st| st.course().to_vec())
            .unwrap_or_default()
    })
}

/// Decline the open challenge (the racer drives off).
#[wasm_bindgen]
pub fn street_decline() -> Result<(), JsValue> {
    get(|s| s.street_decline())
}

/// The city's drivable lanes for the map, flattened: `[n, x0, y0, …, x(n-1), y(n-1)]` per lane
/// (parking lanes left out). Fetch once per session; empty on the flat world.
#[wasm_bindgen]
pub fn map_lanes() -> Result<Vec<f32>, JsValue> {
    get(|s| {
        let mut out = Vec::new();
        if !s.has_city() {
            return out;
        }
        for lane in &s.sim().world().network().lanes {
            if lane.kind == wp_world::network::LaneKind::Parking || lane.polyline.len() < 2 {
                continue;
            }
            out.push(lane.polyline.len() as f32);
            for p in &lane.polyline {
                out.extend_from_slice(&[p[0], p[1]]);
            }
        }
        out
    })
}

/// Debug / e2e: put vehicle `entity` (stopped) at `(x, y)` facing `yaw` (0 = +Y), on the
/// ground. Not recorded in replays.
#[wasm_bindgen]
pub fn debug_place_vehicle(entity: u32, x: f32, y: f32, yaw: f32) -> Result<(), JsValue> {
    with(|s| {
        let sim = s.debug_sim_mut()?;
        let z = sim.world().map().ground_height([x, y], None);
        sim.place_on_ground(EntityId(entity), [x, y], yaw, 0.0, Some(z))
            .map_err(|e| e.to_string())
    })
}

/// Debug / e2e: park street racer `entity` idle at the Loop 9 rest stop `name` (as between
/// laps, two minutes; a logged move). Errors without street racers, for an unknown rest stop or a
/// racer that is not roaming.
#[wasm_bindgen]
pub fn debug_street_park_at_rest_stop(entity: u32, name: &str) -> Result<(), JsValue> {
    with(|s| s.street_park_at_rest_stop(EntityId(entity), name))
}

/// The Loop 9 rest stops as JSON: `[{ name, pos, heading_rad }]` (`[]` on maps without them).
#[wasm_bindgen]
pub fn rest_stops_json() -> Result<String, JsValue> {
    get(|s| {
        let v: Vec<_> = s
            .sim()
            .world()
            .rest_stops()
            .map(|p| serde_json::json!({ "name": p.name, "pos": p.pos, "heading_rad": p.heading_rad }))
            .collect();
        json(&v)
    })
}

/// The city's repair shops as JSON ([`garage_visit::GarageSite`] array: name, position,
/// radius, `Legit` / `Crooked`, district). `[]` without a city or an economy run.
#[wasm_bindgen]
pub fn garages_json() -> Result<String, JsValue> {
    get(|s| json(&s.garages()))
}

/// The open garage visit as JSON ([`garage_visit::GarageOffer`]: shop, wanted level, G's
/// `RepairAll` quote at that shop, cash, whether "Repair" is possible), or `null`. A visit
/// opens when the player's car stops inside a garage with no pursuit on; the car is held on
/// the brakes until `garage_repair()` or `garage_leave()`.
#[wasm_bindgen]
pub fn garage_visit_json() -> Result<String, JsValue> {
    get(|s| json(&s.garage_visit()))
}

/// "Repair" in the open garage visit: pays the shop's `RepairAll` from the ledger, repairs the
/// car in place (replay op `Repair`) and releases the brakes. Returns
/// [`garage_visit::GarageRepairReport`] JSON; an error (nothing changed) without a visit, with
/// nothing to repair, or when the player cannot afford it.
#[wasm_bindgen]
pub fn garage_repair() -> Result<String, JsValue> {
    with(|s| s.garage_repair().map(|r| json(&r)))
}

/// "Leave" the open garage visit without repairing (no-op without one).
#[wasm_bindgen]
pub fn garage_leave() -> Result<(), JsValue> {
    get(|s| s.garage_leave())
}

/// Bet on yourself in the current race while the countdown runs: `label` from the picker's
/// `odds[].label` (`win:player`, `podium:player`, `h2h:player>vic`), `stake` in credits. The
/// career economy takes the stake (rep tier cap, cash, debt). Returns the ticket
/// ([`race::BetCard`]) as JSON; an error when betting is locked, closed or the stake is out of
/// bounds.
#[wasm_bindgen]
pub fn race_bet(label: &str, stake: f64) -> Result<String, JsValue> {
    with(|s| s.race_place_bet(label, stake as i64).map(|t| json(&t)))
}

/// Picker-race betting (venue `City`) on the current session's account: `{"max": credits,
/// "lock": reason or null}`. Street-racer challenge stakes are in `street_status_json`.
#[wasm_bindgen]
pub fn race_bet_status() -> Result<String, JsValue> {
    get(|s| {
        let (max, lock) = s.bet_status();
        json(&serde_json::json!({ "max": max, "lock": lock }))
    })
}

/// The city's regulars with their records ([`race::RegularCard`] array), strongest first.
#[wasm_bindgen]
pub fn roster_json() -> String {
    json(&race::regular_cards())
}

/// The regulars' records as text, for the testbed's local save.
#[wasm_bindgen]
pub fn roster_state() -> String {
    race::roster_state_text()
}

/// Restore the regulars' records saved with `roster_state()`.
#[wasm_bindgen]
pub fn roster_state_load(text: &str) -> Result<(), JsValue> {
    race::roster_state_load(text).map_err(err)
}

// ----- touge battles ------------------------------------------------------------------------

fn touge_setup(
    map: &str,
    direction: &str,
    daylight: &str,
    surface: &str,
    opponent: &str,
) -> Result<wp_race::touge::TougeSetup, String> {
    wp_race::touge::TougeSetup::from_keys(map, direction, daylight, surface, opponent).ok_or_else(
        || format!("unknown touge battle {map}:{direction}:{daylight}:{surface}:{opponent}"),
    )
}

/// The touge menu as JSON ([`touge::TougeCard`] array): the four passes with length, climb,
/// K-rail stretches and their challenger and boss (name, title, personality, car).
#[wasm_bindgen]
pub fn touge_list_json() -> String {
    json(&touge::touge_cards())
}

/// The wager menu for one battle as JSON (`wp_race::touge::WagerMenu`): rep and tier, whether
/// racing this opponent, side bets and pink slips are open (and the rep tier each needs),
/// the largest stake, payouts and rep. Read from the account the battle will use: the career's
/// when `career` (the battle starts as a career run) and a career is loaded, else the
/// session's (fresh accounts start at 0 rep: everything locked). Keys: `map` (pass id),
/// `up|down`, `day|night`, `dry|rain`, `challenger|boss`.
#[wasm_bindgen]
pub fn touge_menu_json(
    map: &str,
    direction: &str,
    daylight: &str,
    surface: &str,
    opponent: &str,
    career: bool,
) -> Result<String, JsValue> {
    let setup = touge_setup(map, direction, daylight, surface, opponent).map_err(err)?;
    let career_acct = if career {
        career::career_account()
    } else {
        None
    };
    let acct = match career_acct {
        Some(a) => a,
        None => with_session_opt(|s| {
            Ok::<_, JsValue>(s.and_then(|s| s.run().map(|r| r.account().clone())))
        })?
        .unwrap_or_else(|| garage::with_economy(|e| e.open_account(run::PLAYER_ACCOUNT).0)),
    };
    Ok(json(&touge::menu(&acct, &setup)))
}

/// Scenario JSON of a touge battle (for `init` or `career_start_run`), and remember `wager`
/// for it: `{"kind":"none"}`, `{"kind":"side_bet","stake":N}` or `{"kind":"pink_slip"}`
/// (taken at the first tick on the player's account; refused wagers show in
/// `touge_result_json`). The player starts behind the NPC.
#[allow(clippy::too_many_arguments)]
#[wasm_bindgen]
pub fn touge_scenario_json(
    map: &str,
    direction: &str,
    daylight: &str,
    surface: &str,
    opponent: &str,
    car: &str,
    mode: &str,
    wager_json: &str,
    autopilot: bool,
) -> Result<String, JsValue> {
    let setup = touge_setup(map, direction, daylight, surface, opponent).map_err(err)?;
    let wager: wp_race::touge::Wager = if wager_json.is_empty() {
        wp_race::touge::Wager::None
    } else {
        serde_json::from_str(wager_json).map_err(err)?
    };
    let sc = touge::touge_scenario(&setup, car, parse_mode(mode).map_err(err)?, autopilot)
        .map_err(err)?;
    touge::set_next_wager(wager);
    Ok(json(&sc))
}

/// Start a touge battle as a plain (non-career) session: [`touge_scenario_json`] + `init`.
/// Returns the entity ids (player first, then the NPC).
#[allow(clippy::too_many_arguments)]
#[wasm_bindgen]
pub fn touge_start(
    map: &str,
    direction: &str,
    daylight: &str,
    surface: &str,
    opponent: &str,
    car: &str,
    mode: &str,
    wager_json: &str,
    autopilot: bool,
) -> Result<Vec<u32>, JsValue> {
    let sc = touge_scenario_json(
        map, direction, daylight, surface, opponent, car, mode, wager_json, autopilot,
    )?;
    let sc: simcli::Scenario = serde_json::from_str(&sc).map_err(err)?;
    Ok(install(Session::new(sc, false).map_err(err)?))
}

/// The touge panel as JSON ([`touge::TougeReport`]): setup, wager taken or refused, the
/// decision (who won, who went through the wall), the NPC car's condition when its race ended
/// and the settlement (side bet, purse, rep, car won / lost). `null` outside a touge battle.
#[wasm_bindgen]
pub fn touge_result_json() -> Result<String, JsValue> {
    get(|s| {
        json(
            &s.race()
                .and_then(|r| r.touge())
                .map(touge::TougeRun::report),
        )
    })
}

/// Testbed only: set the plain session account's rep, to try rep-gated touge battles and
/// wagers without a career. Not recorded in replays; refused during a career.
#[wasm_bindgen]
pub fn touge_debug_set_rep(rep: f64) -> Result<(), JsValue> {
    with(|s| {
        if s.is_career() {
            return Err("not during a career".to_string());
        }
        s.debug_run_mut()
            .ok_or("this session has no account")?
            .debug_set_rep(rep as i64);
        Ok(())
    })
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

/// Debug / e2e: put a police helicopter on `entity` now, `offset_m` from it at altitude
/// (`PoliceSystem::debug_launch_helicopter`). Returns the unit id, or an error unless the racer
/// is pursued. Not recorded in replays.
#[wasm_bindgen]
pub fn debug_launch_helicopter(entity: u32, offset_m: f32) -> Result<u32, JsValue> {
    with(|s| {
        s.debug_sim_mut()?
            .police_mut()
            .debug_launch_helicopter(EntityId(entity), offset_m)
            .map(|u| u.0)
            .ok_or_else(|| "racer is not pursued".to_string())
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

/// Debug / e2e: aim `entity` at a parked car so it knocks it at `speed_mps` (placed 5.5 m off
/// the car's flank, facing it). `prop` < 0 picks the parked car nearest the vehicle. Returns
/// the prop index (into `props_static()`). Not recorded in replays.
#[wasm_bindgen]
pub fn debug_knock_prop(entity: u32, prop: i32, speed_mps: f32) -> Result<u32, JsValue> {
    with(|s| {
        let pick = (prop >= 0).then_some(prop as usize);
        s.debug_knock_prop(EntityId(entity), pick, speed_mps)
            .map(|i| i as u32)
    })
}

/// Debug / e2e: put `entity` on the ground at `[x, y]` facing `yaw` (0 = +Y) at `speed_mps`
/// (e.g. inside a garage from `garages_json()`). Not recorded in replays.
#[wasm_bindgen]
pub fn debug_place(entity: u32, x: f32, y: f32, yaw: f32, speed_mps: f32) -> Result<(), JsValue> {
    with(|s| {
        s.debug_sim_mut()?
            .place_on_ground(EntityId(entity), [x, y], yaw, speed_mps, None)
            .map_err(|e| e.to_string())
    })
}

/// Floats per prop in `props_static()` ([`props::PROP_STRIDE`]).
#[wasm_bindgen]
pub fn prop_stride() -> u32 {
    props::PROP_STRIDE as u32
}

/// Floats per record in `props_moved()` ([`props::MOVED_STRIDE`]).
#[wasm_bindgen]
pub fn prop_moved_stride() -> u32 {
    props::MOVED_STRIDE as u32
}

/// Every city prop (parked cars, cones) as the sim placed it, 11 floats each:
/// `[kind (0 parked car, 1 cone), x, y, z, qx, qy, qz, qw, half_x, half_y, half_z]` (box
/// centre in world m; half extents in the prop frame). Fetch once per session; also restarts
/// `props_moved()` from these poses.
#[wasm_bindgen]
pub fn props_static() -> Result<Vec<f32>, JsValue> {
    get(|s| {
        PROP_SYNC.with(|p| p.borrow_mut().reset(s.sim()));
        props::static_list(s.sim())
    })
}

/// Props whose pose changed since the last call, 8 floats each:
/// `[prop index, x, y, z, qx, qy, qz, qw]`. Usually empty: only knocked (or nearby, woken)
/// props move. Call once per frame after `step()`.
#[wasm_bindgen]
pub fn props_moved() -> Result<Vec<f32>, JsValue> {
    get(|s| PROP_SYNC.with(|p| p.borrow_mut().moved(s.sim())))
}

/// The sim's current pose of prop `index`: `[x, y, z, qx, qy, qz, qw]` (empty if there is no
/// such prop). Automation (e2e: drawn pose vs sim pose).
#[wasm_bindgen]
pub fn prop_pose(index: u32) -> Result<Vec<f32>, JsValue> {
    get(|s| {
        s.sim()
            .prop_pose(index as usize)
            .map(|(p, q)| vec![p[0], p[1], p[2], q[0], q[1], q[2], q[3]])
            .unwrap_or_default()
    })
}

/// Floats per record in `parked_wrecks()` ([`props::WRECK_STRIDE`]).
#[wasm_bindgen]
pub fn parked_wreck_stride() -> u32 {
    props::WRECK_STRIDE as u32
}

/// Dented parked cars (owner playtest 2026-10-21), ascending prop index, 12 floats each:
/// `[prop index, lattice awake (0/1), health, glass regions shattered, sx, sy, sz, ox, oy,
/// oz, 0, 0]`. Its entity id is `parked_entity_base() + prop index`; that id works with
/// `vehicle_info`, `lattice_nodes`, `lattice_rest`, `lattice_regions`, `lattice_beams`,
/// `lattice_strain` and `panel_states` (always empty) like a vehicle's. The lattice is in the
/// car's chassis frame; the prop body frame (its pose from `props_static()` /
/// `props_moved()`) is `chassis ⊙ s + o`. The model is `vehicle_info(entity).car_id`. A prop
/// enters this list on the tick it is first hit hard enough to dent and never leaves it.
#[wasm_bindgen]
pub fn parked_wrecks() -> Result<Vec<f32>, JsValue> {
    get(|s| props::wreck_list(s.sim()))
}

/// Entity id of the dented parked car of prop 0 (`wp_sim::PARKED_ENTITY_BASE`); prop `i` is
/// this plus `i`.
#[wasm_bindgen]
pub fn parked_entity_base() -> u32 {
    wp_sim::PARKED_ENTITY_BASE
}

/// Body-region indices (like `vehicle_info().regions`) of a dented parked car's shattered
/// glass, ascending.
#[wasm_bindgen]
pub fn parked_wreck_shattered(entity: u32) -> Result<Vec<u32>, JsValue> {
    with(|s| {
        let w = s
            .parked_wreck(EntityId(entity))
            .ok_or_else(|| format!("unknown parked car {entity}"))?;
        let mut out: Vec<u32> = w
            .damage
            .shattered_regions()
            .iter()
            .map(|r| pack::region_index(*r) as u32)
            .collect();
        out.sort_unstable();
        Ok(out)
    })
}

/// Floats per agent in `traffic_agents()`.
pub const AGENT_STRIDE: usize = 7;

/// Floats per record in `police_tools()`.
pub const TOOL_STRIDE: usize = 11;

/// Floats per record in `police_tools()`.
#[wasm_bindgen]
pub fn police_tool_stride() -> u32 {
    TOOL_STRIDE as u32
}

/// Police tools on the road, as oriented boxes, 11 floats each:
/// `[kind, x, y, z, qx, qy, qz, qw, hx, hy, hz]` (centre, orientation, half extents; box local
/// +X across the road). `kind` 0 = roadblock barrier, a physics body whose pose changes when
/// it is hit; 1 = spike strip, lying flat on the road (local +X along the strip).
#[wasm_bindgen]
pub fn police_tools() -> Result<Vec<f32>, JsValue> {
    get(|s| {
        let sim = s.sim();
        let mut out = Vec::new();
        for b in sim.roadblock_barriers() {
            out.push(0.0);
            out.extend_from_slice(&b.pos);
            out.extend_from_slice(&b.rot);
            out.extend_from_slice(&b.half_extents_m);
        }
        let map = sim.world().map();
        for st in sim.spike_strips() {
            let c = [(st.a[0] + st.b[0]) * 0.5, (st.a[1] + st.b[1]) * 0.5];
            let d = [st.b[0] - st.a[0], st.b[1] - st.a[1]];
            let half_len = 0.5 * (d[0] * d[0] + d[1] * d[1]).sqrt();
            // Display only (not simulation), so std atan2 is fine here.
            let (sn, cs) = (d[1].atan2(d[0]) * 0.5).sin_cos();
            let z = map.ground_height(c, None) + 0.03;
            out.extend_from_slice(&[1.0, c[0], c[1], z, 0.0, 0.0, sn, cs, half_len, 0.22, 0.03]);
        }
        out
    })
}

/// Tyre deflation per vehicle, 4 floats each (FL, FR, RL, RR; 0 inflated … 1 flat), same order
/// as `vehicle_states()`: the worse of spike-strip punctures and crash damage to the wheel.
#[wasm_bindgen]
pub fn tire_punctures() -> Result<Vec<f32>, JsValue> {
    use wp_contracts::DamageSim;
    get(|s| {
        let sim = s.sim();
        let mut out = Vec::with_capacity(sim.vehicles().len() * 4);
        for v in sim.vehicles() {
            let spikes = sim.spike_puncture(v.id);
            let crash = v.damage.handling_effects().tire_puncture;
            for i in 0..4 {
                out.push(spikes[i].max(crash[i]));
            }
        }
        out
    })
}

/// Wheels running on their rims, 4 floats per vehicle (FL, FR, RL, RR; 1 = a fully flat tyre
/// in ground contact, else 0), same order as `vehicle_states()`. For the rim-scrape sound and
/// sparks ([`wp_vehicle::vehicle::Vehicle::rim_contact`]).
#[wasm_bindgen]
pub fn tire_rim_contact() -> Result<Vec<f32>, JsValue> {
    get(|s| {
        let sim = s.sim();
        let mut out = Vec::with_capacity(sim.vehicles().len() * 4);
        for v in sim.vehicles() {
            out.extend(v.vehicle.rim_contact().map(|r| r as u8 as f32));
        }
        out
    })
}

/// Civilian traffic agents (empty without a city), 7 floats each:
/// `[x, y, z, yaw, flags, agent_id, model]` with flags = bits 1 physical LOD, 2 brake lights,
/// 4 hazard lights, 8 yielding to a siren, 16 turn signal left, 32 turn signal right,
/// 64 aggressive driver, plus `variant << 8` (colour variant 0..8, drawn once at spawn).
/// `agent_id` is the stable `wp_world` id; `model` indexes `traffic_models()` and is
/// `wp_world::traffic::traffic_model_index(agent_id, n)` — the same model (and variant) the
/// agent keeps when promoted to a physical car (whose `vehicle_info().car_id` is
/// `traffic_models()[model]`). `model` is 0 with one or no physical traffic model.
#[wasm_bindgen]
pub fn traffic_agents() -> Result<Vec<f32>, JsValue> {
    get(|s| {
        let mut out = Vec::with_capacity(s.sim().world().agent_count() * AGENT_STRIDE);
        let world = s.sim().world();
        for a in world.agents() {
            let flags = (a.lod == wp_world::Lod::Physical) as u32
                | (a.brake_lights as u32) << 1
                | (a.hazard_lights as u32) << 2
                | (a.yielding as u32) << 3
                | ((a.turn_signal < 0) as u32) << 4
                | ((a.turn_signal > 0) as u32) << 5
                | ((a.personality == wp_world::traffic::Personality::Aggressive) as u32) << 6
                | (a.variant as u32) << 8;
            out.extend_from_slice(&[
                a.pos[0],
                a.pos[1],
                a.pos[2],
                a.yaw,
                flags as f32,
                a.id as f32,
                world.traffic_model_of(a.id) as f32,
            ]);
        }
        out
    })
}

/// Floats per record in `traffic_draw()` / `police_draw()` ([`lod::DRAW_STRIDE`]).
#[wasm_bindgen]
pub fn lod_draw_stride() -> u32 {
    lod::DRAW_STRIDE as u32
}

/// Draw records for every civilian agent at any LOD (see [`lod::traffic_draw`]), 11 floats each:
/// `[agent id, entity (0 = none), x, y, z, qx, qy, qz, qw, flags, model]`; flags as in
/// `traffic_agents()`. Kinematic agents are seated on the ground the way the sim places a
/// promoted car, so the browser can swap instanced and full meshes without a jump.
#[wasm_bindgen]
pub fn traffic_draw() -> Result<Vec<f32>, JsValue> {
    get(|s| lod::traffic_draw(s.sim()))
}

/// Draw records for every police unit, bound or not (see [`lod::police_draw`]), 11 floats each:
/// `[unit id, entity (0 = none), x, y, z, qx, qy, qz, qw, flags, model]`, flags 1 helicopter,
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

/// Car ids of the physical traffic models as a JSON array, in model-index order (the
/// `model` of `traffic_agents()`); `[]` when traffic stays kinematic. The city-chase preset
/// uses simcli's `CAR_MIX_TRAFFIC`. Fetch once per session.
#[wasm_bindgen]
pub fn traffic_models() -> Result<String, JsValue> {
    get(|s| {
        let ids: Vec<&str> = s
            .sim()
            .traffic_cars()
            .iter()
            .map(|c| c.def.id.as_str())
            .collect();
        json(&ids)
    })
}

/// Car ids of the multi-model police fleet as a JSON array, in fleet-index order (the
/// `model` of `police_units()`); `[]` when the session uses one police model
/// (`Sim::set_police_car`, then `model` is -1). Fetch once per session.
#[wasm_bindgen]
pub fn police_models() -> Result<String, JsValue> {
    get(|s| {
        let ids: Vec<&str> = s
            .sim()
            .police_fleet()
            .iter()
            .map(|m| m.car.def.id.as_str())
            .collect();
        json(&ids)
    })
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

/// Painted road markings of the session's city (empty without one) as a triangle soup,
/// `[x, y, z]` per vertex in sim space, already lifted onto the banked road surface
/// (`wp_world::markings::marking_mesh`). `yellow` picks centre lines and median-side edge
/// lines, otherwise white paint. Fetch once per session.
#[wasm_bindgen]
pub fn road_markings(yellow: bool) -> Result<Vec<f32>, JsValue> {
    get(|s| {
        if !s.has_city() {
            return Vec::new();
        }
        let mesh = wp_world::markings::marking_mesh(s.sim().world().map());
        let m = if yellow { &mesh.yellow } else { &mesh.white };
        let mut out = Vec::with_capacity(m.indices.len() * 9);
        for t in &m.indices {
            for i in t {
                out.extend_from_slice(&m.positions[*i as usize]);
            }
        }
        out
    })
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
        // Dented parked cars have no hinged panels.
        if s.parked_wreck(EntityId(entity)).is_some() {
            return Ok(Vec::new());
        }
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

// ----- crash replay (lead) -----------------------------------------------------------------

/// The big crash detected since the last call, as JSON ([`crash_replay::CrashMark`]), or
/// `null`. Live sessions only (`null` during any replay).
#[wasm_bindgen]
pub fn crash_take() -> Result<String, JsValue> {
    get(|s| json(&s.crash_take()))
}

/// Crash thresholds JSON ([`crash_replay::CrashTuning`]).
#[wasm_bindgen]
pub fn crash_tuning_json() -> Result<String, JsValue> {
    with_live(|s| Ok(json(&s.crash().tuning())))
}

/// Set crash thresholds from JSON (missing fields keep their defaults).
#[wasm_bindgen]
pub fn crash_tuning_set(tuning_json: &str) -> Result<(), JsValue> {
    let t: crash_replay::CrashTuning = serde_json::from_str(tuning_json).map_err(err)?;
    with_live(|s| {
        s.crash_mut().set_tuning(t);
        Ok(())
    })
}

/// Turn the replay buffer + crash detection on or off for this session (on by default; the
/// testbed turns it off for benches). Off drops the buffer.
#[wasm_bindgen]
pub fn crash_detect_enable(on: bool) -> Result<(), JsValue> {
    with_live(|s| {
        s.crash_mut().set_enabled(on);
        Ok(())
    })
}

/// Ticks of the buffered snapshots (oldest first) of the live session.
#[wasm_bindgen]
pub fn crash_buffer_ticks() -> Result<Vec<f64>, JsValue> {
    with_live(|s| {
        Ok(s.crash()
            .snapshot_ticks()
            .into_iter()
            .map(|t| t as f64)
            .collect())
    })
}

/// Play crash `seq` (the latest crash, from `crash_take`) again: parks the live session and
/// installs a playback session for `impact − 3 s .. impact + 2 s` (`replay_play` /
/// `replay_status` / `step` drive it like any replay). Returns
/// [`crash_replay::CrashReplayInfo`] JSON. The live session is not stepped or changed until
/// [`crash_replay_stop`].
#[wasm_bindgen]
pub fn crash_replay_start(seq: u32) -> Result<String, JsValue> {
    if crash_replay_active() {
        return Err(err("a crash replay is already playing"));
    }
    let (replay, info) = with(|live| {
        let mark = live
            .crash()
            .last()
            .filter(|m| m.seq == seq)
            .cloned()
            .ok_or_else(|| format!("no crash #{seq} in this session"))?;
        live.crash_replay(&mark)
    })?;
    SESSION.with(|s| {
        let mut slot = s.borrow_mut();
        let live = slot.replace(replay);
        PROP_SYNC.with(|p| {
            p.borrow_mut()
                .reset(slot.as_ref().expect("installed").sim())
        });
        PARKED.with(|p| *p.borrow_mut() = live);
    });
    Ok(json(&info))
}

/// Whether a crash replay is playing (the live session is parked).
#[wasm_bindgen]
pub fn crash_replay_active() -> bool {
    PARKED.with(|p| p.borrow().is_some())
}

/// End the crash replay: the parked live session comes back exactly as it was. Returns
/// `{"tick": n, "hash": "<hex>"}` of the live session, or `null` when no replay was playing.
#[wasm_bindgen]
pub fn crash_replay_stop() -> String {
    let Some(live) = PARKED.with(|p| p.borrow_mut().take()) else {
        return "null".into();
    };
    let out = serde_json::json!({
        "tick": live.tick(),
        "hash": format!("{:016x}", live.sim().state_hash()),
    })
    .to_string();
    SESSION.with(|s| {
        PROP_SYNC.with(|p| p.borrow_mut().reset(live.sim()));
        *s.borrow_mut() = Some(live);
    });
    out
}

/// Run `f` on the live session (the parked one during a crash replay).
fn with_live<T>(f: impl FnOnce(&mut Session) -> Result<T, String>) -> Result<T, JsValue> {
    with_session_opt(|s| match s {
        Some(s) => f(s).map_err(err),
        None => Err(err("call init() first")),
    })
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
