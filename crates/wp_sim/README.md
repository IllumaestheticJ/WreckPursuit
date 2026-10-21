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
- Slot reserves: police stop spawning at 64 − 16 vehicles, traffic at 64 − 24, parked cars at
  64 − 32.
- `police()` exposes the police system; `examples/police_probe.rs` prints a chase timeline.

## Dentable parked cars (prototype, `parked_link.rs`)
City parked cars are rigid box props (`props.rs`): cheap, shovable, never dented. With
`set_parked_car(car)` (after `set_world`; simcli scenario flag `parked_damage: true`, on in the
browser free drive) a parked-car prop that a vehicle is about to hit is swapped for a full
`VehicleKind::Parked` vehicle at the prop's pose (handbrake on, no driver):
- Swap test, each tick before the vehicle step: parry shape distance from every vehicle's
  chassis collider to nearby parked-car props ≤ 0.5 m + closing speed × 0.15 s. A car driving
  past parallel does not swap; one closing in does, a few ticks before contact.
- The prop's body and collider are disabled while the vehicle stands in (wheel probes skip
  disabled colliders, since the broad phase still holds them until the next physics step).
- Release: at rest (< 0.1 m/s, < 0.1 rad/s) for 1 s with no other vehicle within 12 m → the
  vehicle is despawned and the prop put back, fixed, where the car stopped. A dented car's
  `DamageModel` is stored by prop index and given back if it is swapped in again.
- Limits: at most `MAX_PARKED_VEHICLES` (8) at once and none once the sim has 64 −
  `PARKED_SLOT_RESERVE` (32) vehicles; past either limit the prop stays a box.
- `parked_vehicle_count()`, `damaged_parked_count()` for diagnostics.
- Cost (release, `cargo run --release -p simcli --example city_frame_cost -- <city>
  [--parked-damage]`, all systems, 2026-10-21): Port Vesper 0.78–0.94 ms → 0.81–1.13 ms per
  60 Hz frame (run-to-run noise, no parked car swapped in); 12×12 grid 0.76 → 0.99 ms with 2
  parked cars swapped in (≈ 0.1 ms each, i.e. one more stationary vehicle).

Prototype gaps: a stored wreck is a box again while nobody is near (collider shape and mass of
the box, not the dented hull); the testbed does not draw parked-car props at all, so a parked
car shows up only while it is swapped in; the car model is the traffic car, not a per-slot
variety.

## Known gaps
- Contacts reach `wp_damage`, but damage is still the prompt-01 stub until workstream 03 merges.
- Static geometry is boxes only (`add_static_box`, TB-3); simcli/testbed scenarios do not create them yet.
- Chassis collider is the box from `wp_vehicle::build_body`; a lattice hull comes with 03.
- Economy is not wired in.

## Tests

```sh
cargo test -p wp_sim
```
