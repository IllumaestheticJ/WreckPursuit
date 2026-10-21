# Car tune (G10) integration notes

The free per-car tune screen from `docs/reference/wreckfest-parity.md` → "Tune screen (G10):
proposed design": four 5-step sliders (suspension soft↔stiff, gear ratio short↔long,
differential open↔locked, brake balance rear↔front), 3 = stock, separate from the paid mod tree.

- **Workstream:** game menus + money/career + vehicle (owner request 2026-10-20)
- **Branch:** `claude/loving-tesla-bpkq1m` (on top of `lead/round12`)
- **Crates / folders touched:** `wp_vehicle` (`tune` module), `data/tune/`, `wp_sim` (spawn),
  `apps/simcli` (entity field), `wp_economy` (garage state), `wp_wasm` (career + race exports),
  `apps/ui-kit` (garage Tune page), `apps/testbed-web` (wiring, developer row, e2e)
- **Contract version built against:** 0.3.0 (no contract edit; see WP-4 below)

## How it flows

1. **Data:** `data/tune/steps.toml` — the step tables (the parity doc's first proposal, owned by
   H). Step 3 must be the identity (checked at parse). `[cars.<id>]` overrides a row; the two
   heavy trucks (`drayman_cargo`, `hauler_1500`) get a narrower spring range (×0.85 … ×1.16).
2. **Mapping:** `wp_vehicle::tune::apply(&mut VehicleDef, &mut DiffSetup, CarTune, &TuneSteps)`;
   `tune::tuned(def, tune) -> (VehicleDef, DiffSetup)` uses the embedded tables.
   | slider | fields |
   |---|---|
   | suspension | `spring_n_per_m`, `arb_n_per_m` × scale; both dampers × √scale |
   | gearing | `final_drive` × scale |
   | diff | driven axle `AxleDiff` (rear for RWD and AWD, front for FWD; AWD centre untouched): 1 open, 2 stock clutch × (0.5 preload, 0.6 ramps), 3 stock, 4 preload × 2.5 with ramps ≥ 0.75 / 0.60, 5 locked. A non-clutch stock diff (helical, viscous, active, open) scales a 60 N·m / 0.40 / 0.25 base clutch |
   | brake balance | `bias_front` + offset, clamped 0.30–0.80 |
   A slider at 3 leaves its fields bit-for-bit untouched; an all-stock tune builds the car file
   exactly (tested), so goldens and replays of untuned cars do not move.
3. **Spawn:** `Sim::spawn_vehicle_tuned(car, pos, yaw, tune)` (`spawn_vehicle` = stock). The
   tuned differential goes in through `VehicleExtras::diffs`, so `differentials.toml` no longer
   overrides it and the car keeps its id. `SimVehicle::tune` records what it was built with.
4. **Scenarios:** simcli `EntitySpec::tune` (RON `tune: (suspension: 5, gearing: 5, diff: 4,
   brake_balance: 2)`, missing sliders stock, not written when stock). `build_sim` spawns with
   it, so the browser session, races and replays (the scenario rides in the replay header) all
   apply it.
5. **Save:** `OwnedCar::tune` (`#[serde(default)]`, not written when stock: old saves load,
   profile version unchanged). `Economy::set_tune(acct, car, tune)` — free, no ledger line,
   car must be in the garage, steps 1..=5.
6. **Career:** `Career::set_tune`, `career_set_tune(uid, json)`, `CarView::tune`;
   `Career::start_run` puts the car's tune on the first entity.
7. **Quick runs (outside the career):** the last tune per car id lives in the viewer's
   `localStorage` (`wp.tune.v1`, ui-kit `garage/tune.ts`); free drive and city chase put it on
   the player entity, races use the new `race_start_tuned(id, car, mode, autopilot, tune_json)`
   (`race_start` = stock).
8. **UI:** garage → Cars → **Tune** (next to Mods): four range sliders (Tab between them, arrow
   keys step), step names (Soft … Stiff, Short … Long, Open / Light LSD / Stock / Tight LSD /
   Locked, Rear … Front), a line under each saying what it does, **Reset to stock**, a summary,
   and a "tuned" badge on the car card. Changes save at once and apply from the next run. The
   testbed developer panel has a **car tune** row (same four sliders, for the car in the car
   selector) for A/B work.

## Exposes
- `wp_vehicle::{CarTune, tune::{apply, tuned, tuned_axle, TuneSteps, tune_steps, TUNE_STEPS, TUNE_STOCK}}`
- `wp_sim::{CarTune, Sim::spawn_vehicle_tuned, SimVehicle::tune}`
- simcli `EntitySpec::tune`
- `wp_economy::{CarTune, Economy::set_tune, OwnedCar::tune}`
- wasm: `career_set_tune(uid, tune_json)`, `race_start_tuned(...)`; `CarView.tune` in the career view JSON
- ui-kit: `CareerApi.setTune`, `garage/tune.ts` (slider labels, `normalizeTune`, `tuneSummary`, quick-run storage)

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| WP-4 | Move `CarTune { suspension, gearing, diff, brake_balance: u8 }` (1..=5, default 3; serde: missing fields = 3, `deny_unknown_fields`) into `wp_contracts`, plus `TUNE_STEPS` / `TUNE_STOCK` | Server, net spawn and replays must apply the same tune. Today there are two identical copies: `wp_vehicle::CarTune` (sim) and `wp_economy::CarTune` (save; the economy does not depend on `wp_vehicle` / rapier), converted in one place (`wp_wasm::career::vehicle_tune`) | `wp_vehicle` and `wp_economy` re-export the contract type and delete their copies (same JSON, saves keep loading); `wp_wasm` drops the conversion; `wp_net` can then add the tune to the join/spawn message | requested (implementation ready; contract not edited: lead-owned) |

## Changes outside my crates
None beyond the list above; each is the minimal glue for its step: `OwnedCar` literals in
`wp_net/src/server.rs`, `wp_wasm/src/{garage,run}.rs` and `wp_economy/tests/properties.rs` gained
`tune: Default::default()`; `EntitySpec` literals in simcli `fuzz.rs`, `examples/city_frame_cost.rs`
and `wp_wasm/src/scenario.rs` likewise.

## Tests
- `wp_vehicle` `tune::tests` (9): stock = identity on 4 cars (RWD, FWD, AWD, truck); every slider
  changes only its fields; diff steps on a clutch and a helical stock diff; FWD changes the front;
  bias clamp; truck override; parse rejects a non-identity stock step; serde defaults.
- `wp_sim` `tuned_spawn_applies_the_tune`: stock tune = same state hash as `spawn_vehicle`;
  the owner's tune changes final drive, locks the rear and the run's hash.
- `wp_economy` `tests/tune.rs` (3): free, no ledger line; invalid steps / unknown car / impounded
  car refused; save round-trip and old saves read as stock.
- `wp_wasm` `a_tuned_car_spawns_tuned_and_the_tune_is_saved`: free, saved, refused mid-run, the
  run's vehicle is built tuned.
- ui-kit `garage/tune.test.ts` (5); testbed `e2e/tune.mjs` (10 checks, headless Chromium):
  garage page, keyboard steps, reload, career run carries it, reset, developer row → free drive
  and race, no page errors.

## Known gaps
- **Net:** `wp_net` spawns stock cars (the join message has no tune) — waits on WP-4.
- **Stats:** the Tune page has no stat bars yet (the design leaves them for later; the parity
  metrics could feed acceleration / top speed / cornering).
- **Gamepad:** the garage screen has no pad navigation (true for all its tabs); the sliders are
  keyboard and mouse/touch operable.
- **Parity suite:** `crates/wp_vehicle/examples/parity.rs` still has its private `apply_tune`
  (same numbers for RWD; it reduces diffs to the contract `DiffDef`). It can switch to
  `wp_vehicle::tune::tuned` + `VehicleExtras::diffs`.
- The step values are the parity doc's first proposal; H owns tuning them.
