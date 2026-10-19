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

## Known gaps
- Contacts reach `wp_damage`, but damage is still the prompt-01 stub until workstream 03 merges.
- Static geometry is boxes only (`add_static_box`, TB-3); simcli/testbed scenarios do not create them yet.
- Chassis collider is the box from `wp_vehicle::build_body`; a lattice hull comes with 03.
- Economy is not wired in.

## Tests

```sh
cargo test -p wp_sim
```
