# wp_wasm

**Owned by workstream 04 (browser testbed).**

wasm-bindgen surface of the simulation for `apps/testbed-web`. A thin `lib.rs` over
`session::Session` (plain Rust, unit-tested natively). A session is a **simcli scenario (format
v2)**: the sim is built by `simcli::run::build_sim`, scripted entities use
`simcli::run::driver_input`, scenario results come from `simcli::run`, and recordings are
canonical `replay.bin` files that `simcli replay` verifies.

| Module | What |
|---|---|
| `lib.rs` | exported functions (full ABI table: `docs/integration/testbed.md`) |
| `session.rs` | `Session`: fixed-step loop, live / scripted / playback, op + input log, per-tick hashes, drift check |
| `scenario.rs` | free-drive scenario generator, `.ron` parsing, cruise-speed override (all simcli v2) |
| `pack.rs` | packed typed-array layouts, beam strain, region energy |
| `street.rs` | NPC street racers (lead): 16 roaming `wp_opponent` drivers (4 at each of levels 2–5), scenario entities `street_<level>_<name>` so replays carry them; horn challenge, A→B race to a player-picked finish (never-give-up race AI as in picker races), wagers through the run's ledger; tests in `street_tests.rs` |
| `run.rs` | the player's economy (lead): drained events → G's `EconomySession`, end-of-run `RunReport`, "Repair & continue"; tests in `run_tests.rs` |
| `career.rs` | the browser career (machine G): persistent account, garage actions, career runs with the career car's mods and saved lattice damage, `career_*` exports; tests in `career_tests.rs` (see `docs/integration/economy.md`) |
| `lots.rs` | car lots in the career: lot stock / buy / tyres exports, and the **lot visit** session (`career_visit_lot`: today's stock parked in rows with its wear and tyres, the player's car at the gate, fences and fixtures per lot kind); tests in `lots_tests.rs` |
| `garage_visit.rs` | garage visits (lead): the city's `Garage` POIs as repair shops (legit / crooked, district); stopping in one holds the car and offers the shop's `RepairAll`; detection in `Session::check_garage`, tests in `run_tests.rs` |
| `touge.rs` | touge battles: scenario builder (locked weather, rep-gated wager), off-the-mountain breach (drop, guard rail, corridor) → total loss, early decision, settlement on the run or career account, pink-slip car (economy snapshot + full lattice) into the career garage; `touge_*` exports; tests in `touge_tests.rs` |
| `police.rs` / `garage.rs` / `audio.rs` | police read-outs + siren rule, live repair bill, audio director glue |
| `data.rs` + `build.rs` | every file in `data/{cars,lattices,tires,materials,modes}` embedded at build time, also as a simcli `MemData` bundle |

## Build / test

```sh
cargo test -p wp_wasm                                   # native unit tests (replay round trip, scenarios, layouts)
cargo build --target wasm32-unknown-unknown -p wp_wasm  # wasm check
cd apps/testbed-web && npm run wasm                     # wasm-pack release package into src/wasm/pkg
```

Dependencies are wasm32-clean: `simcli` (lib), `wp_world`, `wp_weather`, `wp_race` + `wp_opponent` (street races, `src/race.rs`), `postcard` (no_std + alloc), `serde_json`, `wasm-bindgen`.

Street races (workstream 15): `race_list`, `race_start`, `race_info`, `race_standings`,
`race_course`, `race_result_json`; a scenario named `race_<id>` gets a `race::RaceRun` (AI field
before each tick, judge after it, buy-in/payout on the player's run). See
`docs/integration/race.md` → Step 2. Honk-to-start spots: `race_spots()` (JSON) and
`race_spot_honked()` (`src/race_spots.rs`; the check runs in `Session` after every live tick,
read-only); `race_start` keeps the current map when the race runs there.

## Perf
Real vehicle + damage, headless Chromium (SwiftShader), release wasm: 0.09 / 0.62 / 3.0 ms of
sim per 60 Hz frame at 1 / 8 / 24 cars; 0.27 ms in the 12×12 city with ~190 traffic agents. See
`docs/integration/testbed.md`.
Weather (`weather.rs`): +0.1 ms of sim per frame in heavy rain on the flat world (0.24 vs 0.14 ms,
1 car, 62 500-cell grid); `weather_info` 0.005 ms, `weather_grid` 0.8 ms (called at 1 Hz).
Street racers (`street.rs`, 13 AI cars: 8 out by day, 13 at night, owner row 152), native
release, city free drive, sim ms per 60 Hz frame with them (median of 3, 2026-10-22): grid noon
5.12, grid 23:00 3.02, Vesper noon 7.57, Vesper 23:00 4.69 (before SR-3: 7.40 / 5.07 / 9.41 /
7.74). What did it, in order of weight: far roaming racers drive the sim's light model and drop
out of the police racer views (`Sim::set_light_when_far`; Vesper 23:00 racers' share +5.1 ->
+2.3 ms), the live session hashes the state once per frame instead of every tick
(`LIVE_HASH_EVERY`, ~0.06 ms per car per frame saved, every session), off-shift racers sleep
(`Sim::set_dormant`, ~0.1 ms per parked car). The street AI is 0.25–0.35 ms of it.
`cargo test --release -p wp_wasm --lib street_perf -- --ignored --nocapture --test-threads=1`.

## Known gaps
- Instant crash replay (`crash_replay.rs`, `docs/integration/crash-replay.md`): a live session snapshots its `Sim` every 2 s (~6 ms native in a night run); the last ~1.5 s of a replay accepted early is extrapolated from the last logged inputs.
- Sandbox runs (car selector, `startFreeDrive(car)`) keep the end-of-run account in memory only; career runs (`career.rs`) persist it (the testbed saves the career to `localStorage`).
- Replays of a career run whose car has mods or started damaged do not verify (the replay rebuilds the stock, pristine car).
- The end-of-run economy (`run.rs`) keeps the account in memory only (lost on reload); the lattice blob in `DamageReport` is empty (the live sim keeps the dents).
- Garage shop kind (legit / crooked) comes from the `docs/design/CITY.md` shop list by name, else the district, until the POIs carry it (CITY.md request C3). A legit shop's report of a wanted car is only a `ShopReported` ledger line: police heat does not react yet. Garage visits offer `RepairAll` only (no per-item repair: the sim repairs the whole car).
- Region energy / strain are proxies until contract request TB-2 lands; `intents()` is empty (TB-5).
- `simcli replay` cannot verify city recordings until the CLI turns on `RunOptions::load_city`; race recordings also need the race (AI kinds, meet report) re-applied, which only the browser's playback does.
- Street racers: their inputs and moves are logged, so recordings (and crash replays) of street sessions verify in the browser's playback; the playback has no street AI, so it shows no offer / race HUD / map icons, and `simcli replay` drifts (city recordings, and it does not mark the racers `AiRacer`, as with picker races). Wagers are `Adjustment` ledger lines (see `docs/integration/testbed.md`, SR-2…3). The log grows ~1 050 input events/s with 16 racers (~36 KB/s of `replay.bin`; fewer with 8–13): every AI steer change is an event.
- Street racers off shift (row 152) are parked in a lockup on flat terrain away from roads and buildings (`street::lockup_slots`; past the east edge on a flat city): replays have no despawn / spawn op, so the cars stay in the sim, asleep (`Sim::set_dormant`, op `Dormant`) once they stand still with the player > 100 m away.
- Out of play (`out_of_play.rs`): only the player's car (entity 0) is put back on the road after deep water or the map edge; the multiplayer server has no such recovery yet.
- `cargo test -p wp_wasm` takes ~2 min in debug (real physics).
