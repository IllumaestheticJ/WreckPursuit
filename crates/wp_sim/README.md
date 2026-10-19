# wp_sim

**Owned by workstream 12 (integration lead).**

The fixed-step game loop. `Sim::new(SimConfig)`, `spawn_vehicle(&LoadedCar, pos, yaw)`, `set_input`, `set_assists`, `reset_vehicle`, `step_fixed()` (one `PHYSICS_DT` tick), `advance(frame_dt) -> alpha`, `vehicle_states()`, `drain_events()`, `state_hash()`.

Per tick: world clock → vehicles → Rapier step (gravity -Z, static ground slab with top at z = 0) → damage contacts → `HandlingEffects` back to vehicles → police → events.

`state_hash` = FNV-1a over tick + per vehicle (entity order) quantised pos (1e-4 m), rot (1e-5), velocities (1e-4), rpm (1), gear, health (1e-4).

## Known gaps
- Vehicles are not Rapier bodies yet, so no contacts reach damage. Arrives with prompt 02/03 merges.
- Economy is not wired in.

## Tests

```sh
cargo test -p wp_sim
```
