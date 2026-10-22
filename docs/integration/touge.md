# Touge integration notes

- **Workstream:** touge passes, touge battles, side bets and pink slips (project thread "touge")
- **Branch:** `claude/project-thread-rgdat5` (built on the career branch
  `claude/project-thread-elq8kz`, merged in; land that PR first)
- **Crates / folders owned:** `wp_world::touge`, `wp_race::touge`, `wp_economy::touge`,
  `wp_wasm::touge` (+ `touge_tests.rs`), `data/touge/`, `data/events/touge.toml`,
  `apps/testbed-web/src/ui/touge.tsx`
- **Contract version built against:** 0.1.0 (no contract changes)

## Exposes
- `wp_world::touge`: `TOUGE_IDS`, `spec(id)`, `is_touge(id)`, `generate(&TougeSpec) -> TougeBuild
  { net, krails, track }`, `generate_builtin(id)`; `World::touge(id, cfg)` (K-rails become parked
  props). `PropKind::KRail`.
- `wp_race::touge`: `MAPS`, `ROSTER`, `TougeSetup` (keys / scenario name
  `race_touge:<map>:<up|down>:<day|night>:<dry|rain>:<challenger|boss>`), `plan_course`,
  `Boundary`, `TougeBattle`, `BattleOutcome`, `TougeTable` (`data/events/touge.toml`), `Wager`,
  `wager_menu`, `place`, `settle`. `RaceCity::Touge(id)` in the catalog.
- `wp_economy::touge`: `WonCar`, `Economy::take_pink_slip_car`, `Economy::lose_pink_slip_car`.
- wasm: `touge_list_json`, `touge_menu_json`, `touge_scenario_json`, `touge_start`,
  `touge_result_json`, `touge_debug_set_rep`. Props kind code 2 = K-rail.
- simcli: `CitySpec::Preset { name: "touge_*" }` loads a pass.
- `gen-city --preset touge` (or one id) writes `data/city/<id>.greybox.glb` / `.b64.txt` /
  `.road.bin`.

## Needs
- Career progression (`wp_economy::progression` on `claude/project-thread-elq8kz`): every gate
  goes through `require_unlocked(Feature::TougeRaces | BossRaces | TougeBetting | PinkSlips)`,
  stakes through `max_bet(BetVenue::Touge)` / `place_bet` / `settle_bet`, rep through
  `award_rep`. No thresholds are defined here.
- Garage parts (`claude/project-thread-thv5w5`): adds `OwnedCar.parts`. When both land,
  `Economy::take_pink_slip_car` needs `parts: None` (or the parts derived from the won car's
  damage) in its `OwnedCar` literal.

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| — | none | | | |

## Changes outside my crates
- `wp_damage::DamageModel::declare_total_loss` (sets `total_loss`; used when a car goes through
  the K-rails, since the fall alone may not wreck it before the battle is decided).
- `apps/simcli`: `load_world` knows the touge presets.
- `wp_wasm`: `scenario.rs` city spec for touge ids, `props.rs` kind code, `race.rs` (`RaceRun`
  hosts a `TougeRun`), `run.rs` (`touge_place` / `touge_settle` / `debug_set_rep`), `career.rs`
  (`career_account`, won lattice into `profile.lattices`), `session.rs` (debug placement).
- `apps/ui-kit/src/menu/GameMenu.tsx`: "Touge battles" main-menu entry.
- `apps/testbed-web`: touge button + picker, K-rail rendering (`render/props.ts`), battle panel.
- `tools/city-import`: `gen-city --preset touge`.
- `wp_sim` untouched: walls are ordinary fence decor and K-rails ordinary static props.

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| Generate one pass (debug build) | cloud container | ≈ 0.1 s | — | load time |

## Known gaps
- The opponent car won by pink slip arrives with its economy damage snapshot and its full
  lattice state in a career; outside a career it lands in the session account only.
- The boundary check is lateral distance from the centre line (+1.5 m past the wall) or 4 m
  below the road; a car balanced on top of the K-rails is not out until it falls.
- K-rail mass (600 kg per 2 m section) is lighter than real concrete so a committed off at
  speed goes through; a glancing hit only shoves them.
- Greybox meshes are generated files; regenerate with `gen-city --preset touge` after editing
  a spec.

## Lead feedback
