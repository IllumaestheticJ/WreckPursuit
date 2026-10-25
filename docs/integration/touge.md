# Touge integration notes

- **Workstream:** touge passes, touge battles, side bets and pink slips (project thread "touge")
- **Branch:** `claude/project-thread-rgdat5` (PR #49); ported to `lead/round12` on
  `lead/touge-port` without the career and free-roam commits underneath it (see
  `docs/integration/touge-port.md`)
- **Crates / folders owned:** `wp_world::touge`, `wp_race::touge`, `wp_economy::touge`,
  `wp_wasm::touge` (+ `touge_tests.rs`), `data/touge/`, `data/events/touge.toml`,
  `apps/testbed-web/src/ui/touge.tsx`
- **Contract version built against:** 0.3.1 (no contract changes)

## Exposes
- `wp_world::touge`: `TOUGE_IDS`, `spec(id)`, `is_touge(id)`, `generate(&TougeSpec) -> TougeBuild
  { net, rails, track }`, `generate_builtin(id)`, `EdgeKind` / `EdgeSpan` (`track.edges`),
  `COURSE_EXTENT_M`; `World::touge(id, cfg)` (guard rails become parked props).
  `PropKind::GuardRail` + `PropKind::breakaway_mps` / `GUARD_RAIL_BREAK_MPS` (`wp_world::props`).
- `wp_race::touge`: `MAPS`, `ROSTER`, `TougeSetup` (keys / scenario name
  `race_touge:<map>:<up|down>:<day|night>:<dry|rain>:<challenger|boss>`), `plan_course`,
  `Boundary`, `TougeBattle`, `BattleOutcome`, `TougeTable` (`data/events/touge.toml`), `Wager`,
  `TougeGates` / `Gate` (`touge.toml [gates]`), `wager_menu`, `place`, `settle`. `Boundary::check_near`
  / `locate`, `BreachKind::{OffCourse, OffTheEdge}`, `TRACK_WINDOW_M`, `TRACK_JUMP_M`.
  `RaceCity::Touge(id)` in the catalog; `catalog::race_for_scenario` resolves touge names.
- `wp_economy::touge`: `WonCar`, `Economy::take_pink_slip_car`, `Economy::lose_pink_slip_car`,
  `Economy::touge_result` (rep as `RaceRep`, boss purse as `RacePurse`).
- wasm: `touge_list_json`, `touge_menu_json`, `touge_scenario_json`, `touge_start`,
  `touge_result_json`, `touge_debug_set_rep`. Props kind code 3 = guard rail (2 = K-rail, unused by the passes).
- simcli: `CitySpec::Preset { name: "touge_*" }` loads a pass.
- `gen-city --preset touge` (or one id) writes `data/city/<id>.greybox.glb` / `.b64.txt` /
  `.road.bin`.

## Needs
- Career progression (`wp_economy::progression`, being ported on `lead/career-port`). Until it
  lands the gates are rep thresholds in `data/events/touge.toml [gates]` (the career's tier
  numbers) and side bets are cash adjustments on the account ledger (`Economy::adjust`, the
  street racers' wager path). When it lands: `TougeGates::check` → `require_unlocked(Feature::…)`,
  `TougeGates::max_stake` → `max_bet(BetVenue::Touge)`, the stake / payout adjustments →
  `place_bet` / `settle_bet`, `Economy::touge_result` → `award_rep` + `award_credits`, and
  drop `[gates]`.

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| — | none | | | |

## Changes outside my crates
- `wp_damage::DamageModel::declare_total_loss` (sets `total_loss`; used when a car goes off
  the mountain, since the fall alone may not wreck it before the battle is decided).
- **2026-10-24 touge overhaul:** `wp_sim::props` (anchored props: a `PropKind` with
  `breakaway_mps` stays fixed until a mover within 3 m of its face closes on it faster than
  that; unit test), `wp_world::props` (`PropKind::GuardRail`, `breakaway_mps`),
  `wp_world::surface::drawn_spacing` (touge maps draw and collide their terrain at its own 2 m
  resolution; every other city unchanged), `wp_wasm::props` (kind code 3),
  `apps/testbed-web/src/render/props.ts` (instanced W-beam + posts), `wasm/layout.ts`,
  `wp_wasm::race` (`RaceRun::new`: a rain touge plans the AI's corner and brake limits with
  `wp_race::touge::Surface::ai_grip`).
- `apps/simcli`: `load_world` knows the touge presets.
- `wp_wasm`: `scenario.rs` city spec for touge ids, `props.rs` kind code, `race.rs` (`RaceRun`
  hosts a `TougeRun`), `run.rs` (`touge_place` / `touge_settle` / `debug_set_rep`), `career.rs`
  (`career_account`, won lattice into `profile.lattices`), `session.rs` (debug placement).
- `apps/ui-kit/src/menu/GameMenu.tsx`: "Touge battles" main-menu entry.
- `apps/testbed-web`: touge button + picker, guard rail rendering (`render/props.ts`), battle panel.
- `tools/city-import`: `gen-city --preset touge`.
- `wp_world::World::set_parked_cars` keeps non-car props (guard rails) of the parked list, after
  the cars (round12's `Sim::set_world` reseats parked cars on terrain and replaced the whole
  list, which dropped the K-rails and shifted prop indices).

## Career money and police (2026-10-25, playtest "touge runs don't count towards career money")
- Challenger wins now pay a purse (`data/events/touge.toml [challenger] purse = 3500`; boss
  stays 12 000) through `award_credits` on the battle's account, which on a career run is the
  career's. Before, a challenger win with no side bet paid nothing.
- Touge passes have no police at all: `touge_scenario` sets `systems =
  "vehicle,damage,world,net"` (`TOUGE_SYSTEMS`), so no units, heat or radio callouts.
  `Session::build` keeps the player's economy run for touge scenarios even without police.
- Career: a touge run starts with 0 heat in the session and `finish_run` leaves
  `carry.heat` as it was (the police never saw the pass). Touched outside the touge files:
  `wp_wasm/src/session.rs` (run gate), `wp_wasm/src/career.rs` (heat in/out).

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| Generate one pass (debug build) | cloud container | ≈ 0.1 s | — | load time |
| Generate one pass, 2026-10-24 overhaul (debug build, 3 m terrain + forest) | cloud container | ≈ 0.4 s | — | load time |
| Greybox `.glb` per pass, 2026-10-24 (3 m terrain drawn at full resolution, pines) | — | 3.8–8.3 MB (was 0.4–0.7) | — | download |

## Known gaps
- The opponent car won by pink slip arrives with its economy damage snapshot and its full
  lattice state in a career; outside a career it lands in the session account only.
- The boundary is 4 m below the road beside the car or `COURSE_EXTENT_M` (20 m) beyond the paved
  edge, each car judged within 40 m of road of where it last was; a car hung up on the verge
  past a broken rail is not out until it falls (the judge's stuck rule ends it).
- Guard rail breakaway is a closing-speed gate (6.5 m/s into the face), not a force model:
  rails do not bend, and once loose a 120 kg section is a light box the car carries along.
- AI drivers do not know about drops or rails. Autopilot-vs-boss sweep (8 battles, all passes
  both ways, release): dry 0/8 off the mountain; rain 8/8 before `Surface::ai_grip` (the race AI
  planned wet corners at dry grip), 3/8 after (0.7; 0.6 was no better and slower). The rest of
  the wet offs are AI recoveries (reversing out of a runoff through its rail): a
  `wp_opponent` follow-up.
- Open drops are kept off the outside of bends
  (where cars run wide) so the AI rarely meets one, but a big AI mistake can now cost it the
  battle.
- Runoff aprons read as dirt (`surface_at`) but are drawn with the 3 m terrain's zone colour;
  there is no gravel-trap drag beyond the dirt tyre model.
- The Ashpeak open-world touge (Crowfoot Pass Road) is a city road and keeps the city's road
  furniture; it is not built by `wp_world::touge`.
- Greybox meshes are generated files; regenerate with `gen-city --preset touge` after editing
  a spec.

## Lead feedback
