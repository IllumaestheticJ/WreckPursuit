# wp_sim

**Owned by workstream 12 (integration lead).**

The fixed-step game loop. `Sim::new(SimConfig)`, `spawn_vehicle(&LoadedCar, pos, yaw)`, `set_input`, `set_assists`, `reset_vehicle`, `step_fixed()` (one `PHYSICS_DT` tick), `advance(frame_dt) -> alpha`, `vehicle_states()`, `drain_events()`, `state_hash()`.

Per tick: world clock → vehicles → Rapier step (gravity -Z, static ground slab with top at z = 0) → damage contacts → `HandlingEffects` back to vehicles → police → events.

`state_hash` follows hash spec v1 (`HASH_SPEC_VERSION`, full spec in `docs/integration/harness.md`): FNV-1a over version, tick, clock, vehicle count and every `entity_hashes()` value in ascending entity order; each entity hash covers quantised pose, velocities, rpm, gear, health, wheels and lattice nodes.

## Test surface (workstream 10)
- `Systems` / `SystemId` registry and `Sim::set_systems` — run a subset of systems (`Systems::parse("vehicle,world")`); default `Systems::ALL`.
- `Sim::last_tick_event_count`, `Sim::node_positions(id)`, `Sim::entity_hashes`.
- Scenario-level tests live in `apps/simcli` (see `docs/TESTING.md`).

## Known gaps
- Vehicles are not Rapier bodies yet, so no contacts reach damage. Arrives with prompt 02/03 merges.
- Economy is not wired in.

## Tests

```sh
cargo test -p wp_sim
```
