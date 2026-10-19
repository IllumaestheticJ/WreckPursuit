# Contract Changelog

**Rule:** any change to `crates/wp_contracts` or `schemas/` bumps `wp_contracts::CONTRACT_VERSION`
(semver: breaking → minor while < 1.0, additive → patch) and gets an entry here stating what
changed, why, and which workstreams must update. Only the integration lead edits the contract.
Every session reads this file at start.

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
