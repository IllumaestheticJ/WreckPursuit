# wp_police

**Owned by workstream 06 (police & pursuit AI).**

Heat/bounty, dispatch, pursuit AI, captain tools. **Currently a stub**: `PoliceSystem::step(world, racers, events, dt, out)` does nothing; `WantedLevel` and `RacerView` are placeholders.

## Known gaps
- Everything. `SimEvent::PoliceEvent` payload is opaque bytes until this crate defines it.

## Perf
Stub: negligible. Budget: ≤ 0.5 ms/frame.

## Tests

```sh
cargo test -p wp_police
```
