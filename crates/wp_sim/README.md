# wp_sim

**Owned by workstream 12 (integration lead).**

The fixed-step game loop. `Sim::new(SimConfig)`, `spawn_vehicle(&LoadedCar, pos, yaw)`, `set_input`, `set_assists`, `reset_vehicle`, `place_vehicle(id, pos, yaw, speed)`, `add_static_box(pos, size, yaw)`, `set_mode_table`, `step_fixed()` (one `PHYSICS_DT` tick), `advance(frame_dt) -> alpha`, `vehicle_states()`, `drain_events()`, `state_hash()`.

Per tick: world clock → vehicles (`wp_vehicle` wheel probes against Rapier, forces written to
the chassis bodies) → Rapier step (gravity -Z, static ground slab with top at z = 0) → contact
extraction (`contacts.rs`) → damage contacts → `HandlingEffects` back to vehicles → police →
events.

Contacts: after the Rapier step every chassis collider's contact pairs become
`ContactImpulse`s in that vehicle's chassis space (normal into the vehicle, closing speed from
pre-step velocities, friction direction rebuilt from the sliding velocity). One
`SimEvent::Contact` per touching pair per tick (emitted by the lower entity id) when the pair's
summed normal impulse is at least `CONTACT_EVENT_MIN_IMPULSE_NS` (50 N·s); damage always gets
every contact. Disabling the vehicle system (`set_systems`) also disables the chassis bodies.

`state_hash` follows hash spec v1 (`HASH_SPEC_VERSION`, full spec in `docs/integration/harness.md`): FNV-1a over version, tick, clock, vehicle count and every `entity_hashes()` value in ascending entity order; each entity hash covers quantised pose, velocities, rpm, gear, health, wheels and lattice nodes.

## Test surface (workstream 10)
- `Systems` / `SystemId` registry and `Sim::set_systems` — run a subset of systems (`Systems::parse("vehicle,world")`); default `Systems::ALL`.
- `Sim::last_tick_event_count`, `Sim::node_positions(id)`, `Sim::entity_hashes`.
- Scenario-level tests live in `apps/simcli` (see `docs/TESTING.md`).

## City: police and traffic as vehicles (lead run 5)
- `set_world(World)` also rebuilds police on the city (`PoliceSystem::for_world`).
- `set_police_car(car)`: AI police car units become `VehicleKind::Police` vehicles driven by
  `wp_police` (`police_link.rs`). `set_traffic_car(car)` (after `set_world`): traffic promoted to
  physical LOD becomes `VehicleKind::Traffic` vehicles driven by `wp_world` (`traffic_link.rs`).
- Slot reserves: police stop spawning at 64 − 16 vehicles, traffic at 64 − 24.
- `police()` exposes the police system; `examples/police_probe.rs` prints a chase timeline.

## City features (Port Vesper v3, road format v4)
`set_city_ground` also adds a static collider for every `solid` `Feature` (after terrain, roads,
buildings, rails and barriers, so cities without features keep their exact collider set):
box → cuboid rotated `box_rotation(yaw, pitch)` (pitch about local +X first, then yaw about +Z),
cylinder → Rapier cylinder turned onto +Z, prism → convex hull of footprint × `[base_z, top_z]`
(`feature_shape`). Shapes are grouped by the `FEATURE_TILE_M` (256 m) tile of their centre and by
drivability into compound colliders (`BTreeMap` order: deterministic). Friction 0.8 for drivable
kinds (`drivable()`: decks, ramps, runway / taxiway / apron, piers, platforms, earthworks,
tunnels, rail bridges, ground), 0.5 otherwise. Counters: `CityColliders::{features,
feature_colliders}`. Tests: `tests/features.rs` (wall stops a car + damage, car rests on a 6 m
deck, car climbs a 1:7 ramp onto it) and unit tests in `physics.rs` (ramp top centreline through
`metro::ctx::ramp`'s end points, cylinder / prism / box tops).

Measured (release, native, `cargo test --release -p wp_sim --test features -- --ignored
--nocapture`): 49 504 solid boxes → 72 compound colliders, `set_world` 57 ms in total (145 ms
with one collider per box); one car driving among them 0.030 ms per 240 Hz step vs 0.025 ms
without features (≈ the same with separate colliders: the BVH broad phase copes; compounds
mainly save build time and memory).

## Known gaps
- Contacts reach `wp_damage`, but damage is still the prompt-01 stub until workstream 03 merges.
- Static geometry is boxes only (`add_static_box`, TB-3); simcli/testbed scenarios do not create them yet.
- Chassis collider is the box from `wp_vehicle::build_body`; a lattice hull comes with 03.
- Economy is not wired in.

## Tests

```sh
cargo test -p wp_sim
```
