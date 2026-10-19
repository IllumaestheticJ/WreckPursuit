# simcli

**Owned by workstream 10 (test harness & CI).**

Headless scenario runner. `just sim <file.ron>` or `cargo run -p simcli -- <file.ron> [--data data]` prints `state_hash <hex>` and a JSON summary; exits 1 if an `expect` range fails.

Scenario format (`scenarios/**/*.ron`): `name`, `car`, `mode` (`Arcade`|`Simulation`), `seed`, `duration_s`, optional `time_of_day_h`, `inputs: [(t_s, input: (VehicleInput fields…))]` (held until the next key), `expect: [(metric, min, max)]`. Metrics: `final_speed_mps`, `max_speed_mps`, `distance_m`, `final_health`.

`tests/scenarios.rs` runs every scenario twice: expectations must pass and hashes must match.

## Known gaps
- Single car per scenario; no outputs written to `out/`; no golden hashes yet (prompt 10).

## Tests

```sh
cargo test -p simcli
```
