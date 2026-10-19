# wp_sim

**Owned by workstream 12 (integration lead).**

The fixed-step game loop. `Sim::new(SimConfig)`, `spawn_vehicle(&LoadedCar, pos, yaw)`, `set_input`, `set_assists`, `reset_vehicle`, `step_fixed()` (one `PHYSICS_DT` tick), `advance(frame_dt) -> alpha`, `vehicle_states()`, `drain_events()`, `state_hash()`.

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

## Known gaps
- Contacts reach `wp_damage`, but damage is still the prompt-01 stub until workstream 03 merges.
- No static world geometry besides the ground slab (barriers: contract request TB-3).
- Chassis collider is the box from `wp_vehicle::build_body`; a lattice hull comes with 03.
- Economy is not wired in.

## Tests

```sh
cargo test -p wp_sim
```
