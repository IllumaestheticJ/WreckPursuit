# Playtest fixes: "cars feel slow" + "transmission is sloppy" (`lead/playtest-feel`)

- **Who:** helper for the integration lead, 2026-10-19 (PLAYTEST_LOG rows 3–4).
- **Branch:** `lead/playtest-feel` from `origin/main` + `origin/ws/vehicle` (A's round 6b:
  weather grip hook, converter power-on shifts) merged first.
- **Crates touched:** `wp_vehicle` (gearbox, shift/clutch logic, pedal filter), `wp_sim`
  (props, police reconcile, entity hash — lead-owned, minimal, listed below), `wp_math`
  (`quantize` rounding, lead-owned), `wp_wasm` (example only), `apps/testbed-web`,
  `apps/simcli/golden` ([bless]). Not touched: `wp_contracts`, `data/`, the torque-converter
  model itself.

## 1. "Slow": the game ran in slow motion

Measured first (headless Chromium, `apps/testbed-web/e2e/chase_perf.mjs`; native mirror
`cargo run --release -p wp_wasm --example chase_bench`; profiles from
`e2e/cpu_profile.mjs` and callgrind).

Causes, biggest first:
1. **Lost time on slow frames.** The frame loop capped one frame at 0.1 s of sim, so any frame
   slower than 100 ms dropped time → slow motion. Now 0.25 s (the session's own
   `MAX_TICKS_PER_STEP`).
2. **3,300 dynamic city props.** Every parked car / cone was a dynamic Rapier body with a
   contact pair against the ground slab: ~0.9 ms per 60 Hz frame even asleep, ~23 ms per frame
   for the first 0.5 s (before they fall asleep). Now fixed bodies that turn dynamic when a
   vehicle or debris body comes within 6 m + 0.35 s of travel, and fixed again once asleep with
   nothing near (`crates/wp_sim/src/props.rs`, test `props_are_fixed_until_a_car_comes_and_get_knocked`).
3. **Police car clone every tick.** `reconcile_police_units` cloned the whole police
   `LoadedCar` (lattice included) every tick, even with no unit to spawn: ~0.35 ms/frame.
4. **Replay hash every tick.** The testbed logs `state_hash` per tick for replay.bin; lattice
   nodes dominate. Same result, faster: four cars' FNV chains in lockstep, `quantize` without
   the `round()` libcall (bit-identical over 600k values, test `fast_round_matches_std`).
   Still the largest single sim cost in wasm (see Requests).
5. **Lattice views.** Every car's wireframe and skin were rebuilt every frame (per-beam
   `subarray` allocations, bounding-sphere recompute). Now only when the nodes change.

Determinism: the perf commit is hash-neutral — `simcli hashes` of all 35 scenarios were
byte-identical before and after it.

HUD: `real time 1.00×` line; amber "⚠ slow motion: 0.62× real time" below 0.95 (`src/rtf.ts`,
1 s window, not measured while paused / replaying / benching). Frame breakdown sim / views /
render in the HUD and `window.__wp.app.perf`.

## 2. "Feel slow": the car

- City chase and free drive default to **Lindwurm 4T** (AWD on-demand hot hatch, dual-clutch
  80 ms shifts, 0–100 in 5.4 s, inside its 4.6–5.4 class band) instead of the debug sedan
  (8.3 s, not a roster car). Any car can still be picked in the menu.
- Keyboard throttle: the testbed ramped the key in 0.2 s *and* the sim's keyboard filter ramped
  again over 0.44 s. Now 0.08 s in JS and `filter_pedal` strength 1 = 0.32 s (keyboard 0.7 ≈
  0.23 s); release still 3× faster.

## 3. "Transmission sloppy": the automatic

See `crates/wp_vehicle/README.md` → "Automatic gearbox". In short: load-based schedule with an
anti-hunting band and 1 s reversal lock, kickdown that skips gears, corner hold at the tire
limit, braking downshifts with a 1.5 s hold, and clutch shifts that release → rev-match in
neutral → re-engage in sync (no flare, no clutch re-ramp). Converter cars keep A's under-power
blend; only the schedule is shared.

Tests: `crates/wp_vehicle/src/drivetrain.rs` (phases, load, no hunting at any constant
load/speed, kickdown + corner hold, braking) and `crates/wp_vehicle/tests/gearbox.rs` (constant
throttle sweep on four cars with no downshift, shift count on a standard launch, rev-matched
re-engagement and ≤ 0.11 s drive gap, kickdown, street-car shift times ≤ 250 ms).

Behaviour changes in A's tests (both explained in the test):
`behaviour::all_assists_off_still_drives` 15 → 12 m/s after 6 s at 60% pedal (the box now
short-shifts at part throttle); `metrics::rwd_coupe_metrics` power-oversteer onset bound 0.7 →
0.9 (the old 0.52 came from the old box dumping the clutch at the limiter mid-corner).

Snapshot format 2 → 3 (gearbox shift length, load, last direction, brake hold).

## Changes outside my assigned crates (for the lead)

- `crates/wp_sim/src/props.rs` (new), `lib.rs`: props fixed until approached;
  `Sim::active_prop_count()`; `entity_hashes` lockstep (same values).
- `crates/wp_sim/src/police_link.rs`: clone the police car only when a unit needs one.
- `crates/wp_math/src/hash.rs`: `quantize` rounds without `f64::round` (same values).
- `apps/simcli/golden/*`: blessed in one `[bless]` commit (reasons in its message; 8 of the
  moves predate this branch and come from the ws/vehicle merge).
- `apps/simcli/SCENARIOS.md`: the braking example's "stopped" check accepts −0.5 m/s (the held
  brake engages auto-reverse a moment sooner now).

## Requests

- **Lead (hash spec v4, optional):** the per-tick replay hash is now ~25–35% of the browser's
  sim time, almost all of it FNV over every lattice node of every car. A word-wise hash
  (e.g. 8 bytes per multiply) or hashing a per-car lattice digest that only changes on
  deformation would remove most of it. Needs a `HASH_SPEC_VERSION` bump and a full re-bless.
- **A (vehicle):** your converter work can keep its own shift blend; `Gearbox::auto_shift`
  now takes `&ShiftInputs` (adds `lateral_usage`), and `Gearbox` has new fields in the
  snapshot (format 3). The engagement phases only apply to non-converter cars.
- **H (cars):** please re-run `collect_results.py`; 0–100 moved by −0.1…+0.25 s (table in
  `docs/PLAYTEST_LOG.md`). No car data was changed.
