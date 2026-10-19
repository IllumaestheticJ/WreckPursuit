# PROMPT 01 — Architecture, repo skeleton & shared contracts (RUN FIRST, ALONE)

You are setting up the WreckPursuit repository so that nine other Claude Code sessions can work in parallel without stepping on each other. Read `CLAUDE.md` at the repo root first and obey it. Your output is a compiling Cargo workspace with stub crates, a fully specified `wp_contracts` crate, JSON schemas, example data files, a browser testbed skeleton, and integration docs. Nobody else can start until you push, so prioritize **correct, complete interfaces over any real implementation**. Stubs may return zeros; signatures must be final.

## Deliverables

### 1. Workspace
- `Cargo.toml` workspace with crates listed in `CLAUDE.md`. Rust edition 2021, `rust-toolchain.toml` pinned to current stable.
- Dependencies: `rapier3d` (features: `enhanced-determinism`, `serde-serialize`), `nalgebra`, `serde`, `serde_json`, `toml`, `schemars` (JSON schema generation), `ron`, `thiserror`, `indexmap`, `wasm-bindgen` (only in `wp_wasm`), `tokio` + `tokio-tungstenite` (only in `apps/server` and `wp_net` behind a `native` feature).
- CI: `.github/workflows/ci.yml` running fmt, clippy, test, and a `wasm32-unknown-unknown` build of `wp_wasm`.
- `justfile` (or `Makefile`) with: `just test`, `just wasm`, `just web` (vite dev), `just server`, `just sim <scenario>`.

### 2. `wp_contracts` — the heart of this prompt
Define these exactly (serde + schemars derives on all data types; doc-comment every field with units):

**Constants**
- `PHYSICS_DT: f32 = 1.0/240.0`, `NET_TICK_HZ: u32 = 60`, `VEHICLE_SUBSTEPS: u32 = 2`, `MAX_VEHICLES: usize = 64`.

**Vehicle definition (`VehicleDef`, loaded from `data/cars/*.json`)**
- `id`, `display_name`, `class: VehicleClass {Beater, Compact, Sport, Muscle, Exotic, Truck, PoliceCruiser, PoliceInterceptor, PoliceSuv}`, `analog_of: String` (free text, no trademarks).
- `mass_kg`, `front_weight_frac`, `wheelbase_m`, `track_front_m`, `track_rear_m`, `cg_height_m`, `inertia_kgm2: [f32;3]` (roll, pitch, yaw), `drag_cd_a_m2`, `lift_front`, `lift_rear`.
- `engine: EngineDef { torque_curve_nm: Vec<(rpm:f32, nm:f32)>, idle_rpm, redline_rpm, rev_limit_rpm, inertia_kgm2, engine_brake_nm }`.
- `drivetrain: DrivetrainDef { layout: Fwd|Rwd|Awd{front_split:f32}, gear_ratios: Vec<f32>, final_drive, reverse_ratio, diff_front: Open|Lsd{preload_nm,ramp_power,ramp_coast}|Locked, diff_rear: same, clutch_max_nm, shift_time_s }`.
- `wheels: [WheelDef;4]` ordered FL, FR, RL, RR: `position_m: [f32;3]` (chassis space), `radius_m`, `width_m`, `mass_kg`, `tire_id`, `steerable: bool`, `max_steer_rad`, `ackermann_frac`, `camber_rad`, `toe_rad`, `caster_rad`.
- `suspension: [SuspensionDef;4]`: `spring_n_per_m`, `damper_bump_ns_per_m`, `damper_rebound_ns_per_m`, `travel_m`, `preload_m`, `arb_n_per_m` (per axle, duplicated), `bump_stop_n_per_m`.
- `brakes: BrakeDef { max_torque_front_nm, max_torque_rear_nm, bias_front, handbrake_rear_nm }`.
- `steering: SteeringDef { ratio, rack_speed_rad_per_s, ffb_scale }`.
- `aero_downforce_coeffs`, `lattice_id`, `base_price`, `durability_class: u8`.

**Tire (`TireDef`, `data/tires/*.json`)** — simplified Magic Formula: per axis (lateral, longitudinal) `b, c, d, e`, plus `load_sensitivity` (D scales as `d * (1 - ls * (Fz/Fz0 - 1))`), `fz0_n`, `relaxation_length_m`, `rolling_resistance`, `peak_slip_angle_rad`, `peak_slip_ratio`, `surface_multipliers: BTreeMap<SurfaceType, f32>`, `wear_rate`. `SurfaceType {Asphalt, WetAsphalt, Concrete, Gravel, Grass, Dirt, Curb, Metal}`.

**Materials (`MaterialDef`, `data/materials/*.json`)**: `id`, `density_kg_m3`, `young_mpa`, `yield_mpa`, `plastic_modulus_mpa`, `failure_strain`, `restitution`, `friction`, `kind: Panel|Structural|Glass|Plastic|Cast`.

**Damage lattice (`LatticeDef`, `data/lattices/*.json`)**
- `nodes: Vec<LatticeNode { id:u16, pos_m:[f32;3], mass_kg:f32, pinned_to_chassis: bool, region: BodyRegion }>` where `BodyRegion {FrontBumper, Hood, FrontLeftFender, FrontRightFender, LeftDoorF, LeftDoorR, RightDoorF, RightDoorR, Roof, RearLeftQuarter, RearRightQuarter, Trunk, RearBumper, Windshield, RearGlass, FrameFront, FrameRear, FrameLeft, FrameRight, Cabin}`.
- `beams: Vec<LatticeBeam { a:u16, b:u16, material_id, area_m2, breakable: bool, detach_group: Option<u16> }>`.
- `detachables: Vec<Detachable { group:u16, region, hinge_nodes:[u16;2], break_impulse_ns:f32, mass_kg }>`.
- `components: Vec<ComponentMount { kind: Component, node_ids: Vec<u16>, intrusion_fail_m: f32, intrusion_total_m: f32 }>` with `Component {Engine, Radiator, Battery, FuelTank, SteeringRack, SuspensionFL, SuspensionFR, SuspensionRL, SuspensionRR, WheelFL, WheelFR, WheelRL, WheelRR, Gearbox, Driveshaft, BrakeFL,..., Exhaust, Lights}`.
- `skin_binding: Vec<SkinBind { vertex_index:u32, node_ids:[u16;4], weights:[f32;4] }>` (may be empty for debug cars).

**Mode tables (`ModeTable`, from `data/modes/{arcade,sim}.toml`)**: `impulse_to_lattice_scale`, `plasticity_scale`, `beam_break_scale`, `component_damage_scale`, `component_fail_threshold_scale`, `total_loss_health`, `repair_cost_scale`, `wheel_detach_scale`, `engine_kill_on_total_loss: bool`, `global_health_from_intrusion: bool`.

**Runtime state & IO**
- `VehicleInput { steer:f32 (-1..1), throttle:f32, brake:f32, handbrake:f32, clutch:f32, gear_up:bool, gear_down:bool, horn:bool, headlights:bool, siren:bool, tool_use:u8 }` + `InputSource {Keyboard, Gamepad, Wheel}`.
- `AssistProfile { abs, tcs, esc, steer_speed_sensitivity, countersteer_help, throttle_filter, brake_filter, auto_gears, auto_clutch }` with presets `keyboard_default()`, `gamepad_default()`, `wheel_default()`.
- `VehicleState { pos:[f32;3], rot:[f32;4], lin_vel, ang_vel, wheel_states:[WheelState;4] { omega, steer_rad, compression_m, slip_angle, slip_ratio, fz_n, surface, contact:bool, detached:bool }, engine_rpm, gear:i8, speed_mps, health:f32 (0..1), component_health: BTreeMap<Component,f32>, mode:DamageMode }`.
- `FfbOutput { rack_torque_nm, road_rumble, damper }`.
- `SimEvent` enum: `Contact { a: EntityId, b: EntityId, point, normal, impulse_ns, rel_speed }`, `LatticeDeformed { vehicle, nodes_delta: Vec<(u16,[f32;3])> }` (debug/replay only), `PartDetached { vehicle, group }`, `ComponentDamaged { vehicle, component, health }`, `ComponentFailed{..}`, `WheelDetached{..}`, `TotalLoss{vehicle}`, `TrafficViolation { vehicle, kind: Speeding{over_mps}|RedLight|WrongWay|Collision|Reckless }`, `PoliceEvent{..}` (opaque payload defined by wp_police), `EconomyEvent{..}`.
- `EntityId(u32)`, `PlayerId(u32)`, `Role {Racer, PoliceCaptain, PoliceChase, Spectator}`.
- `TimeOfDay { seconds_since_midnight: f32 }` + `DayPhase {Dawn, Day, Dusk, Night}` helper and `night_factor() -> f32` (0 at noon, 1 at midnight, smooth).
- Netcode message skeletons: `ClientToServer { Input{tick, input}, RoleRequest, CaptainCommand(opaque) }`, `ServerToClient { Snapshot{tick, vehicles: Vec<(EntityId, VehicleState)>, events: Vec<SimEvent>}, Baseline, RoleAssigned }`. Keep `SimEvent` serializable and small.

**Traits each crate implements (define in contracts, implement elsewhere)**
```rust
pub trait VehicleSim { fn step(&mut self, input:&VehicleInput, assists:&AssistProfile, world:&dyn WorldQuery, dt:f32, out:&mut Vec<SimEvent>); fn state(&self)->VehicleState; fn apply_damage_effects(&mut self, eff:&HandlingEffects); fn ffb(&self)->FfbOutput; }
pub trait DamageSim { fn apply_contacts(&mut self, contacts:&[ContactImpulse], mode:&ModeTable, dt:f32, out:&mut Vec<SimEvent>); fn handling_effects(&self)->HandlingEffects; fn node_positions(&self)->&[[f32;3]]; fn repair_estimate(&self, mode:&ModeTable, price_table:&PriceTable)->RepairEstimate; }
pub trait WorldQuery { fn surface_at(&self, p:[f32;3])->SurfaceType; fn speed_limit_at(&self, p:[f32;3])->Option<f32>; fn signal_state_ahead(&self, p:[f32;3], heading:[f32;3])->Option<SignalState>; fn time_of_day(&self)->TimeOfDay; }
```
`HandlingEffects { toe_delta:[f32;4], camber_delta:[f32;4], steer_pull:f32, steer_ratio_scale:f32, power_scale:f32, overheating:f32, brake_scale:[f32;4], wheel_lost:[bool;4], drag_scale:f32, grip_scale:[f32;4] }` — this struct is the entire interface between damage and handling. Make it exhaustive now.

### 3. Schemas & example data
- `cargo run -p wp_contracts --bin gen-schemas` writes JSON Schemas to `schemas/`.
- Example data that validates: `data/cars/debug_sedan.json` (a 1400 kg FWD sedan, 150 kW), `data/cars/debug_coupe.json` (1500 kg RWD, 300 kW), `data/tires/street.json`, `data/materials/{mild_steel,hss,aluminum,pp_bumper,glass}.json`, `data/lattices/debug_sedan.json` (procedurally generated box lattice, ~120 nodes — write a tiny generator binary `gen-box-lattice` that emits it from overall dimensions), `data/modes/arcade.toml`, `data/modes/sim.toml` (arcade = sim × documented multipliers).
- A `wp_contracts::load_car(path)` that validates cross-references (tire ids, lattice ids, material ids) and returns typed errors.

### 4. Stub crates
Each of `wp_math, wp_vehicle, wp_damage, wp_world, wp_police, wp_economy, wp_net, wp_sim, wp_wasm` compiles with a `lib.rs` exposing the public types the other prompts will fill, a `README.md` with a "Owned by workstream NN" line, and one passing placeholder test. `wp_wasm` must build for wasm32 and export `init()`, `step(dt)`, `get_vehicle_state_json()`.

### 5. Apps
- `apps/testbed-web`: Vite + TS + Three.js, loads `wp_wasm` via `wasm-pack`, renders a ground plane and a box per vehicle from `get_vehicle_state_json()`. Keyboard input wired to `set_input_json()`. This is the shell that prompt 04 expands.
- `apps/simcli`: loads a `.ron` scenario (car id, mode, scripted inputs, duration), runs headless, prints a state hash and summary stats. Prompt 10 expands.
- `apps/server`: tokio binary that accepts websocket connections and echoes a tick counter. Prompt 07 expands.

### 6. Docs
- `docs/INTEGRATION.md`: crate map, ownership table, data flow diagram (text), how to run everything.
- `docs/CONTRACT_CHANGELOG.md`: starts with v0.1.0 and the rule that any change bumps the version and is announced here.
- `docs/integration/_TEMPLATE.md`: the file every workstream fills (Exposes / Needs / Contract change requests / Perf numbers / Known gaps).
- `data/cars/README.md`: naming policy (analog names only, e.g. "Kestrel GT" with `analog_of: "late-90s Japanese FR coupe"`), field guide for filling specs.

## Acceptance
- `cargo test --workspace` green; `cargo build --target wasm32-unknown-unknown -p wp_wasm` green; `just web` shows a box that moves when arrow keys are pressed (kinematic stub is fine).
- All example data validates against generated schemas.
- Push to `main`. Write a short `docs/STATUS.md` noting what is stubbed.
