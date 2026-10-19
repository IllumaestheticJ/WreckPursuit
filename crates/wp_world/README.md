# wp_world

**Owned by workstream 05 (city & traffic).**

Road network, signals, traffic, day/night clock. **Currently a stub**: `World` is an infinite flat asphalt plane (no limits, no signals) implementing `wp_contracts::WorldQuery`; `WorldClock` advances `TimeOfDay` by sim time × ratio (default 10).

## Known gaps
- Everything except the clock.

## Perf
Stub: negligible. Budget: ≤ 2 ms/frame for 400 agents native.

## Tests

```sh
cargo test -p wp_world
```
