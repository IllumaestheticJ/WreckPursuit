# Contract Changelog

**Rule:** any change to `crates/wp_contracts` or `schemas/` bumps `wp_contracts::CONTRACT_VERSION`
(semver: breaking → minor while < 1.0, additive → patch) and gets an entry here stating what
changed, why, and which workstreams must update. Only the integration lead edits the contract.
Every session reads this file at start.

## v0.2.0 — plan (not applied yet; decided 2026-10-19 by the lead)

**Still v0.1.0 on the branch.** These are the decisions on every request filed so far, published
early so running workstreams can code toward them. All requesting workstreams (A–I) have now
reported; the bump is the next lead run's first job. Requests filed by 05/07/13/14/15
before then join it.

| Change | From | Decision | Who must update |
|---|---|---|---|
| `GroundQuery` trait (`cast_ray(origin, dir, max_m) -> Option<GroundHit { dist_m, point, normal }>`); `VehicleSim::step` gains `ground: &dyn GroundQuery` | 02 #1 | accepted (breaking) | 02, lead (`wp_sim`) |
| `BrakeDef` doc: bias semantics as implemented by 02, with a worked example | 02 #2, 09 #4 | accepted (doc) | 09 re-checks data |
| `WheelDef::inertia_kgm2: Option<f32>` (default `0.7·m·r²`) | 02 #3 | accepted | 02 |
| `DrivetrainDef::efficiency: Option<f32>` (default 0.85) | 02 #4 | accepted | 02 |
| `DriveLayout::AwdOnDemand { base_front_split, max_rear_split, engage_slip }` | 02 #5, 09 #2 | accepted | 02, 09, 08 |
| `DrivetrainDef::torque_converter: Option<TorqueConverterDef { stall_rpm, stall_torque_ratio, coupling_rpm }>` | 09 #1 | accepted | 02, 09 |
| `VehicleDef::fuel_capacity_l: Option<f32>` (fuel in `mass_kg` at 50 %) | 09 #8 | accepted | 02, 03, 09 |
| `BodyRegion::Accessory` (push bars, body kits) | 09 #7 | accepted in principle; shape with 03 | 03, 08, 09, 04 |
| `WheelState { fx_n, fy_n }` (applied tire-frame forces) | 04 TB-1 | accepted | 02, 04, 07 |
| `DamageSim` region energy / node strain accessors | 04 TB-2 | accepted in principle; shape with 03 | 03, 04 |
| Typed `PoliceEvent` (replaces `{kind, payload}`, gives stable kind codes) | 10 H1, 06 | accepted; designed with 06 | 06, 10, 08 |
| Typed `EconomyEvent` (from `wp_economy::EventReason`) | 08 #1 | accepted; same bump as police/race events | 08, 07, 04 |
| `RepairLine` unscaled-cost doc; `PriceTable::labour_hours: BTreeMap<RepairItem, f32>` | 08 #2 | accepted | 03, 08 |
| `DamageSim::{save_state, restore_state, apply_repair}` | 08 #4 | accepted | 03, 08 |
| `WantedLevel` moves into contracts | 08 #5 | accepted | 06, 07, 08 |
| `DamageSummary { health, total_loss }` next to the estimate | 08 #3 | deferred to 03's merge | 03, 08 |
| `DistrictId` | 08 #6 | deferred to 05's merge | 05, 08 |
| `Sim::debug_intents()` | 04 TB-5 | deferred to 05/06 | 05, 06 |
| `SimEvent::GlassShattered { vehicle, region }`, `SimEvent::RegionDamaged { vehicle, region, severity }` | B #1, #3 | accepted | B, C, F, E |
| `PriceTable::repair_parts_fraction: Option<f32>` | B #2 | accepted (with G #2's labour map) | B, G |
| `SimEvent::TrafficViolation` gains `pos: [f32; 3]` | D #2 | accepted | D, E, F |
| `TrafficAgentSnapshot` + snapshot field | D #3 | accepted; designed with F | D, F, C |
| `wp_math::atan2` (moved from `wp_world::geom`) | D #4 | accepted (lead) | D, A, E |
| Full net protocol message sets into contracts | F NET-1 | accepted in principle; F sends final list | F, C |
| Durable account id separate from per-instance `PlayerId` | F NET-5 | accepted | F, G |
| `StakeCharged` / `RacePayout` economy events | F NET-4 | accepted (inside typed `EconomyEvent`) | G, F |
| Quantised wire types in contracts | F NET-2 | rejected for now (stay in `wp_net`) | — |
| `VehicleSim::teleport` | D #1 | rejected: inherent `Vehicle::set_pose_and_speed` is enough | — |
| Typed `PoliceEvent` + `CaptainCommand` (from `wp_police`; no JSON payloads) | E #2, #3, F NET-3, I H1 | accepted | E, F, G, I |
| `WorldQuery::line_of_sight(a, b) -> bool` | E #4 | accepted (D implements) | D, E |
| Shared read-only road-network trait | E #5 | accepted in principle; shape from D + E | D, E |
| Rear drum brakes + fade inputs | 09 #3 | deferred (post Checkpoint 1) | — |
| Solid axles; roll-centre heights | 09 #5, #6 | deferred (post Checkpoint 2) | — |
| `VehicleState::ffb` | 02 #6 | rejected (FFB is local-only; keep `VehicleSim::ffb`) | — |
| `Systems` registry in contracts | 10 H2 | rejected for now | — |

Done without a contract change (2026-10-19, `wp_sim`): `Sim::add_static_box` (TB-3),
`Sim::set_mode_table` (TB-4), `Sim::place_vehicle`; lattice nodes in the state hash (TB-6, by 10's
hash spec v1); Rapier contacts → `ContactImpulse` → damage + `SimEvent::Contact`.

**Not a contract, but shared format decisions (2026-10-19):** simcli scenario format **v2**
(`apps/simcli/SCENARIOS.md`) is the only scenario format; simcli `replay.bin` v1 is the canonical
replay format (the testbed will write it).

## v0.1.0 — initial contract (prompt 01)

Everything specified in `prompts/01_architecture_contracts.md`, plus these deliberate additions
(flagged so nobody is surprised):

- `VehicleDef::body_size_m: [w, l, h]` — needed for the debug box, lattice generation and the
  fallback chassis collider; the spec listed no overall dimensions.
- `VehicleDef::aero_downforce_coeffs` is `[front, rear]` Cl·A in m² (positive = downforce), added
  on top of `lift_front/lift_rear`.
- `ModeTable::mode` (which mode the table is) and `ModeTable::cabin_intrusion_total_loss_m`
  (prompt 03 asks for the Sim 0.25 m / Arcade 0.5 m cabin limit to live in the tables).
- `ModeTable::wheel_detach_scale` semantics: susceptibility — the wheel mount threshold is
  *divided* by it, so Arcade's 0.5 keeps wheels on longer.
- `HandlingEffects` made exhaustive up front: added `tire_puncture: [f32; 4]` (prompt 06's spike
  strips), `gearbox_health`, `fuel_leak_rate`, `engine_dead`, `lights_broken` (prompt 03's
  component effects). `HandlingEffects::NEUTRAL` / `Default` = undamaged.
- `ContactImpulse` (input of `DamageSim::apply_contacts`): chassis-space point/normal, normal and
  tangential impulse, closing speed, other entity, other mass (`0.0` = static).
- `PriceTable` / `RepairEstimate` / `RepairLine` / `RepairItem` live in contracts because the
  `DamageSim` trait signature needs them (prompt 03 suggested `wp_damage`; this is the promoted
  form). Minimal — 03 and 08 should request refinements.
- `SignalState { Green, Yellow, Red, Off }`; `WORLD_ENTITY = EntityId(u32::MAX)` for contacts with
  static geometry.
- `SimEvent::PoliceEvent { kind: u16, payload: Vec<u8> }` and `EconomyEvent { kind, payload }`
  are opaque until 06/08 request concrete payloads. Same for `ClientToServer::CaptainCommand`
  and `ServerToClient::Baseline`. `ClientToServer::RoleRequest { role }`,
  `ServerToClient::RoleAssigned { player, role, vehicle }`.
- `AssistProfile::ai()` and `AssistProfile::for_source()` presets (prompt 05/06 use `ai()`).
  Note `AssistProfile` lives in contracts (needed by the `VehicleSim` trait) and is re-exported
  by `wp_vehicle`.
- Constants `PHYSICS_TICKS_PER_NET_TICK = 4`, `GRAVITY_MPS2 = 9.81`, wheel index constants
  `WHEEL_FL..WHEEL_RR`.
- Loaders: `load_car`, `load_{vehicle,tire,material,lattice}_def`, `load_mode_table`,
  `parse_*` + `resolve_car` (same validation, for embedded/browser data), `validate_*`,
  typed `LoadError`.
- Serde: data structs `deny_unknown_fields`; enums externally tagged (binary-codec friendly).
- `TimeOfDay::night_factor()` is a smoothstep polynomial (no transcendental functions) so it is
  bit-identical on all targets; `DayPhase` boundaries: Dawn 05–07, Day 07–18, Dusk 18–20,
  Night 20–05.
