# UI kit integration notes

- **Workstream / prompt:** 16 — UI kit (HUD, garage, mod tree, captain map, menus). Machine M.
- **Branch:** `ws/ui`
- **Crates / folders owned:** `apps/ui-kit` (new; TypeScript + Preact, not a Cargo workspace member)
- **Contract version built against:** 0.2.0 (merged `main` after lead run 3). Police types mirror
  `wp_police` (E) until v0.3.0 types them in the contract.

## Exposes

`apps/ui-kit` (`@wreckpursuit/ui-kit`, entry `src/index.ts`; full docs in `apps/ui-kit/README.md`):

| Item | Signature / format | For |
|---|---|---|
| `mountUi` | `mountUi(el, { model, screen?, globalKeys? }) → { unmount, setScreen }` | 04 testbed, later the game client |
| `GameModel` | `{ get(): UiState; subscribe(fn): () => void; send(cmd: UiCommand): void }` | whoever bridges sim/net → UI |
| `UiState`, `UiCommand` | `src/model/types.ts`, `src/model/commands.ts` | provider authors |
| `createMockModel` | mock provider replaying simcli `events.jsonl` + `metrics.csv` + `summary.json` | demo, tests, testbed until a real provider exists |
| `applyEvent(playback, line, ctx)` | pure `SimEvent` → HUD / scanner / captain reducer | reuse in the real provider |
| `parseEventsJsonl`, `parseMetricsCsv`, `parseRun` | simcli output parsers | anyone reading simcli runs in TS |
| Screens | `Hud`, `DamageSilhouette`, `Minimap`, `ReplayControls`, `CaptainMap`, `SCREENS` | hosts composing their own layout |
| Generated contract types | `src/generated/contracts.ts` (`npm run gen:types`) | any TS code (testbed could reuse) |
| `tools/gen-runtime-schemas` | standalone Rust crate: JSON Schemas for runtime contract types → `apps/ui-kit/schemas/runtime/` | lead may want to fold this into `gen-schemas` |
| `tools/record-police` | standalone Rust crate: drives E's real `PoliceSystem` with a scripted racer and records `events.jsonl` + `metrics.csv` + `captain.jsonl` (`CaptainView` at 0.5 Hz) + E's road graph as `map.json` (`npm run gen:police`) | E (a reproducible pursuit trace), anyone needing real police output before `wp_sim` wires police |
| `model/police.ts` | TS mirror of `wp_police::{PoliceEvent, CaptainCommand, UnitOrder, CaptainError, CaptainView, UnitView, RacerSighting, ToolReadiness, Incident}` + `decodePoliceEvent`, `captainCommandBytes` | testbed / client code talking to police before v0.3.0 |
| Demo | `npm run demo` → http://localhost:5180 | everyone |

Wiring into the testbed (04 owns it; nothing edited there):

```ts
import { mountUi, createMockModel } from "../../ui-kit/src";
const hud = mountUi(document.getElementById("hud-overlay")!, { model: createMockModel({ screen: "hud" }), screen: "hud", globalKeys: false });
```

## Needs — data per screen, owner, status

Status: **live** = in the contract and emitted today · **v0.2** = accepted in the v0.2.0 plan ·
**req** = requested below · **mock** = no owner/shape yet, UI mock only.

| Screen | Data | Owner (crate / prompt) | Status | Mock source today |
|---|---|---|---|---|
| HUD | speed, rpm, gear, health, pos, rot | `wp_vehicle` via `VehicleState` (snapshot) | live | `metrics.csv` |
| HUD | redline rpm | `VehicleDef.engine.redline_rpm` (data) | live | `data/cars` |
| HUD | component health / failures, wheel lost, total loss | `wp_damage` → `ComponentDamaged`, `ComponentFailed`, `WheelDetached`, `TotalLoss` | live | events |
| HUD | damage silhouette by `BodyRegion` | `wp_damage` → `RegionDamaged` | contract v0.2.0 ✓, emitted by B (seen in the testbed feed, run 4) | inferred from `Contact` geometry only for older streams without it |
| HUD | detached parts by region | `wp_damage` → `PartDetached` | live, but group ids not mappable → UI-7 | guess (1 front, 2 rear) |
| HUD | glass | `wp_damage` → `GlassShattered` | contract v0.2.0 ✓ | synthetic run |
| HUD | wanted level, pursuit start/end | `wp_police::PoliceEvent::{WantedLevelChanged, PursuitStarted, PursuitEnded}` | live in E (opaque in contract until v0.3.0) | recorded `police_pursuit` + synthetic |
| HUD | heat within level, searching/cooldown progress, units on me | `wp_police` `RacerSighting` (`heat`, `status`, `staleness_s`, `units_assigned`) — today only in `CaptainView` | UI-1 (local racer's sighting for the HUD, networked clients) | recorded `captain.jsonl`; **testbed: live** via `wp_wasm::police_status_json` (single-player, reads `Sim::police()`) |
| HUD | bounty | `wp_economy` `PlayerAccount.bounty` + `BountyGained` | v0.2 (typed EconomyEvent) | mock account + event |
| HUD | minimap roads | `wp_world` `RoadNetwork` (`data/city/*.road.json`) | live (data) | derived fixture |
| HUD | minimap blips (racers, police, heli) | snapshot vehicles + police units | live (vehicles) / UI-3 (units on the wire) | metrics + `CaptainView.units` |
| HUD | race position, lap, checkpoint, gaps | `wp_race` (L): `CheckpointPassed`, `LapCompleted`, `LeadChanged` signals; no live standings/gaps API | UI-6 | derived from distance |
| HUD | assists, damage mode | client settings / `AssistProfile`, `DamageMode` | live | settings |
| HUD | clock | `wp_world` clock → snapshot | req UI-9 | mock |
| Pause | condition summary | as HUD | — | — |
| Settings | bindings, input shaping, a11y, audio | client-local (host persists) | n/a | settings |
| Settings | assist presets | `AssistProfile::{keyboard,gamepad,wheel}_default` | live (mirrored) | — |
| Settings | damage mode table numbers | `data/modes/*.toml`, `consequences.toml` | live (data, mirrored as text) | — |
| Garage | owned cars, damage snapshot, outstanding lines, insurance | `wp_economy` `PlayerAccount` / `OwnedCar` | live in crate, not exposed to TS | mock garage |
| Garage | repair quote (itemised, shop/district/mode, insurance) | `wp_economy::Economy::quote_repair` | live in crate → UI-5 | TS port |
| Garage | value, dealer price, scrap value | `Economy::{current_value, sale_price, scrap_value}` | live in crate → UI-5 | TS port (exact formulas) |
| Mod tree | catalog (slots, lines, tiers, requires, class/mode limits, tradeoff) | `data/mods/*.json` (08) | live (data) | real data |
| Mod tree | before/after perf, class index, bracket | `Economy::preview` | live in crate → UI-5 | TS port of `estimate.rs` |
| Mod tree | durability deltas | `ModEffect::Durability` | live (data) | real data |
| Market | listings, prices | `Economy::market_price`, `data/cars` | live | TS port |
| Lobby | event, bracket, buy-in, purse, mode | race events (15) + `stakes.toml` (08) | mock | mock |
| Lobby | entrants, roles, ready | `wp_net` roles (`RoleRequest`/`RoleAssigned`) | live (roles) / lobby state mock | mock |
| Results | classification, times, DNF/DQ reasons | `wp_race::RacerResult` via `RaceSignal::RaceFinished` (L) | live in L, not in the contract (UI-6) | mock |
| Results | payout lines | typed `EconomyEvent` (`RacePurse`, `CleanRaceBonus`, `RaceRep`, `RaceEntryFee`, `BountyGained`) | v0.2 (08 #1) | mock |
| Impound | cars in pound, fee/day, auction countdown, last capture | `wp_economy` capture / impound | live in crate → UI-5 | TS port of fees |
| Crews map | crews, district ownership/control | `wp_race::{Crew, Territory, DistrictControl}`, `RaceSignal::TerritoryChanged` (L); `DistrictId` deferred to v0.3.0 | live in L, no view API (UI-10) | mock over real zones |
| Police career | rank, XP, unlocks, stats | `wp_economy` cop XP / `cop_level`, unlocks (06) | live (XP) / mock (unlocks) | mock |
| Police career | orders feed, assignment | `PoliceEvent::Radio` + `UnitView.suggestion` (player units) | live in E | events + mock |
| Captain map | roads, zones, POIs | `wp_world` | live (data) | derived fixture |
| Captain map | units (kind, assignment, pos, heading, target, tactic, player-driven, suggestion) | `wp_police::CaptainView.units` | live in E; on the wire = UI-3 | recorded `captain.jsonl`, toy police otherwise |
| Captain map | suspects: level, heat, status, last known, staleness, bounty | `CaptainView.racers` | live in E; on the wire = UI-3 | recorded |
| Captain map | incidents | `CaptainView.incidents` (reports, crashes); `TrafficViolation.pos` (contract v0.2.0 ✓) for non-police runs | live | recorded / events |
| Captain map | heat map | `CaptainView.heat_by_district` (drawn at district centroids) + incidents | live in E | recorded |
| Captain map | tool cooldowns / budgets | `CaptainView.readiness` | live in E | recorded / toy rules |
| Captain map | commands | `wp_police::CaptainCommand` as JSON bytes in `ClientToServer::CaptainCommand` (`captainCommandBytes`); errors `CaptainError` | live in E (typed in v0.3.0) | toy police applies E's rules; recorded runs validate + queue |
| Scanner | dispatch/unit radio, violations, system lines | `PoliceEvent::Radio` (+ other police events), `TrafficViolation`, damage events | live in E | events |
| Replay | transport, markers | testbed replay / simcli `replay.bin` v1 | live (format) | mock transport over fixtures |

Other needs (not contract):

- **10 (harness):** add a `yaw_rad` column to simcli `metrics.csv` (heading is currently derived
  from motion, wrong for a moment after a crash rebound). Optional: `events.jsonl` lines could carry
  the scenario entity id string next to `EntityId` (today joined via `summary.json`).
- **04 (testbed):** mount the HUD overlay (`mountUi(..., { screen: "hud", globalKeys: false })`)
  and build a real provider from `wp_wasm` state + drained events, reusing `applyEvent`.
- **Lead:** consider adding to `justfile` / CI: `ui-demo` (`cd apps/ui-kit && npm run demo`) and a
  CI step `cd apps/ui-kit && npm ci && npm run typecheck && npm test` (and `npm run e2e`, which
  needs the pre-installed Chromium). I did not edit the justfile or workflows (not mine).

### Commands → executor

| `UiCommand` | Executed by | Status |
|---|---|---|
| `navigate`, `back`, `pause`, `resume`, `quitToMenu` | host app | local |
| `settings.*` | host app (persist locally); `damageMode` → `Sim::set_mode_table` next session | local |
| `garage.repair` | `Economy::repair(acct, car, RepairOption, shop, mode)` | crate ready, needs UI-5 / server RPC |
| `garage.buyInsurance` | `Economy::buy_insurance` | crate ready |
| `garage.setActive` | `Economy::set_active_car` | crate ready |
| `mods.install` / `mods.uninstall` | `Economy::{install_mod, uninstall_mod}` | crate ready |
| `market.buy` / `market.sell` | `Economy::{buy_car, sell_car}` | crate ready |
| `lobby.setRole` | `ClientToServer::RoleRequest` | contract ready |
| `lobby.setReady`, `lobby.leave` | `wp_net` lobby messages | F NET-1 (message sets) |
| `results.continue` | host | local |
| `impound.retrieve` | `Economy::retrieve_from_impound` | crate ready |
| `impound.payFines` | economy (fines are charged at capture today; kept for a future "fines owed" flow) | mock |
| `captain` | `PoliceSystem::captain_command(wp_police::CaptainCommand)` via `ClientToServer::CaptainCommand(bytes)` | ready in E |
| `replay.*` | testbed replay player | 04 |

## Contract change requests

**v0.2.0 adopted (2026-10-19):** `GlassShattered`, `RegionDamaged`, `TrafficViolation.pos` and
`WantedLevel` are generated contract types in the UI now (`npm run gen:types`); the UI's
placeholder copies are deleted. **Police:** my earlier invented shapes (old UI-1 / UI-2) are
withdrawn: the UI now decodes E's real `wp_police::PoliceEvent` and sends E's real
`CaptainCommand` (mirrored in `apps/ui-kit/src/model/police.ts`). For the v0.3.0 typed police batch
I ask that E's current variant lists go in as they are, plus the additions below.

| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| UI-1 | The local racer's own `RacerSighting` subset on the wire for the HUD: `level`, `heat`, `status` (`PursuitStatus`), `staleness_s` + the current escape time (or a `SearchStarted { racer, escape_s }` event), `units_assigned`, `bounty` | HUD heat bar, "searching" countdown and units-on-me. Today these exist only inside `CaptainView` (captain role). Events alone give the level and pursuit start/end, not heat or search progress. | 06, 07 | open (v0.3.0) |
| UI-2 | `UnitOrder::Hold { pos: [f32; 2] }` (park / cover a junction) | The captain map's natural "drag unit onto a road" gesture has no order to map to; today it opens the tool radial instead. Optional. | 06 | open, low priority |
| UI-3 | Captain-role snapshot: `CaptainView` (or a quantised subset: units, racers, incidents, tools, readiness, heat_by_district) to the captain client at ~2 Hz, interest-managed | Everything the captain map draws comes from `CaptainView` (verified with a recorded run). | 06, 07 | open (for F's message sets) |
| UI-4 | ~~Heat field for the captain~~ | Withdrawn: `CaptainView.heat_by_district` is enough (drawn at district centroids). | — | withdrawn |
| UI-5 | (wasm surface, not contract) `wp_wasm` exports for the garage: `economy_preview(def_id, mods, mode) → Preview`, `quote_repair(...)`, `current_value`, `sale_price`, `retrieval_fee`, `market_price` (JSON in/out) | Lets the UI drop its ports of `estimate.rs` and the billing rules. | 04, 08 | open (for 04 / 08) |
| UI-6 | Race events into the contract with L's shapes: `RaceSignal::{CheckpointPassed, LapCompleted, LeadChanged, RacerFinished, RacerDnf, RacerDq, RaceFinished { results: Vec<RacerResult> }}`; plus a live standings accessor in `wp_race` (`RaceJudge::standings() -> Vec<{racer, position, progress_m, gap_ahead_s}>`) | HUD race panel (position, gaps) and results screen. L has the data; there is no live position/gap API and nothing reaches the client. | 15, 07 | open (for L) |
| UI-7 | `PartDetached` gains `region: BodyRegion` | Group ids are lattice-specific, so the silhouette cannot map them. (`RegionDamaged` is now emitted by B, seen in the testbed feed.) | 03 | open |
| UI-8 | Typed `EconomyEvent` (v0.3.0, G #1): keep `EventReason` names stable incl. `RaceEntryFee`, `RacePurse`, `CleanRaceBonus`, `RaceRep`, `BountyGained` | Results payout lines map 1:1 to reasons. | 08 | note on an accepted item |
| UI-9 | `ServerToClient::Snapshot` gains `time_of_day: TimeOfDay` (or a low-rate message) | HUD clock. | 05, 07 | open |
| UI-10 | `DistrictId` (deferred to v0.3.0) shared between D's zones and L's `wp_race::DistrictId`, plus a territory view from L: `Vec<{ district, owner: Option<CrewId>, points: BTreeMap<CrewId, u32> }>` and `Crew { id, name, rep, members }` | Crews & territory map (today: data/city zone index + mock owners). | 05, 15, 08 | open |

### Feedback for E (police) from the recorded run

`npm run gen:police` (seed 4242, `CityParams::default()`, night, racer roaming Core at 2.3× the
limit; `apps/ui-kit/fixtures/runs/police_pursuit/`) shows two things worth a look:

1. **`WantedLevelChanged` flaps at thresholds** (seen in a variant of the recording where the racer
   roams the whole city instead of Core, same seed; not in the committed fixture): L2→L1→L2 at
   19.8 / 19.9 / 20.8 s and L4↔L3 four times between 95 and 105 s. A small hysteresis band on level
   *down* would stop the HUD and scanner flickering. (The UI only announces new highs, so it copes.)
2. **Pursuit with no units for about a minute**: in the committed run the racer is `Pursuit` at L5
   and in sight (`staleness_s` ≈ 0) from ~90 s to ~150 s with `units_assigned` = 0 and every car
   on `patrol` (the first wave was wrecked by contact). Units come back as `intercept` once the
   racer is `Searching` (~150 s), and the helicopter launches at ~198 s. My scripted racer is
   kinematic and drives through units (counted as `RacerContact` wrecks), which may be the cause,
   but it may also be the dispatcher not re-sending while the racer stays in sight.

## Changes outside my crates

Live HUD heat / wanted / bounty, pursuit status and the repair bill in the browser testbed
(owner request, 2026-10-19: "wire them into apps/testbed-web with C"). Kept minimal and additive;
**C and the lead, please review**:

| File | Owner | Change |
|---|---|---|
| `crates/wp_wasm/src/garage.rs` (new) | C | `repair_quote_json(entity, request_json)`: the running car's `DamageSim::repair_estimate` (economy `PriceTable` for its class/value) priced by `wp_economy::Economy::quote_repair` (shop, district, mode, wanted markup, insurance, salvage). Read-only. |
| `crates/wp_wasm/src/police.rs` (C's, round 6) | C | **Round 6 consolidation:** my `police_status.rs` / `police_status_json` are gone; C's `police::PoliceStatus` gained four additive fields — `staleness_s`, `search_s` (`los_lost_s`), `escape_s` (level + day phase), `phase` — so one wasm call (`police_status`) feeds the one readout. Unit test extended. |
| `crates/wp_wasm/src/lib.rs` | C | `pub mod garage;` |
| `crates/wp_wasm/Cargo.toml` | C | `wp_economy` dependency (default features; `cargo tree --target wasm32-unknown-unknown` shows no rusqlite). |
| `apps/testbed-web/src/ui/overlay.ts` (new; all testbed-side UI-kit glue lives in `src/ui/`) | C | Mounts the kit's `chaseHud` and `repairBill` overlays on a `createLiveModel()` fed each frame with the drained events + `police_status`. Opens the bill by itself when a chase ends (kit's `ChaseEndDetector`); `ui.lastChaseEnd` records how. |
| `apps/testbed-web/src/ui/police.ts` (new, round 6) | C | `PoliceStatus` type for `police_status` JSON + `toReadout()` onto the kit's `PoliceReadout`. |
| `apps/testbed-web/src/police_hud.tsx` | C | **Removed** (round 6): it was a second heat panel; the kit's chase HUD is now the only one. Its CSS in `index.html` removed too. |
| `apps/testbed-web/src/main.ts` | C | Construct `UiOverlay`, `ui.reset()` on session rebuild, `ui.frame(...)` after `eventsDrain()`, **F** toggles the repair bill, `ui` on `window.__wp`; `PoliceHud` lines removed. |
| `apps/testbed-web/src/wasm/sim.ts` | C | `repairQuote()`; one `policeStatus()` (calls `police_status`, typed by `src/ui/police.ts`). The duplicate `police_status_json` wrapper is gone. |
| `apps/testbed-web/src/settings.ts`, `hud.ts` | C | Layer **Z** "chase readout"; help line mentions **F**. C's "police" layer (also on **Z**) removed. |
| `apps/testbed-web/e2e/run.mjs` | C | After `crash/head_on.ron`: F opens a live, itemised bill; the chase readout is mounted; `police_status` has `escape_s`. City chase: exactly one heat panel (`#ui-chase`). |
| `apps/testbed-web/{package.json,tsconfig.json,vite.config.ts}` | C | `preact` dependency; JSX + `vite/client` types; `resolve.dedupe: ["preact"]`; dev server may serve the repo (the kit imports `../ui-kit` and `data/`). |

The testbed imports `apps/ui-kit/src/live` only (not the kit's index), so no fixtures or mock data
reach its bundle (testbed main chunk 237 kB).

**Verified in the browser (headless Chromium, `npm run e2e` + a manual drive):** after the head-on
crash the bill lists 16 priced lines ($1,868 at a legit shop, more at a crooked one), live from
`wp_economy`.

**Live heat after lead run 5 (2026-10-19):** the readout now reads the real `RacerRecord` through
`police_status_json`, so heat, bounty, escape bar and units are live (no more "—"). In a 60 s
daytime city free drive (debug_sedan, up to 185 km/h) the record shows the police *see* the player
(staleness 0.06–3 s while driving) but heat peaks at **0.76 / 100** and decays to 0, so the readout
stays CLEAR / L0, which is correct for that data. **For D / E:** speeding at 150–185 km/h in view of
the police should probably reach L1 (10 heat) within a few seconds; either the world emits too few
`Speeding` violations or the witnessed-heat value is small. Needs C's city-chase preset (night,
police car model set) to see a pursuit in the browser.

**Round 6 — one readout + end-of-chase bill (2026-10-19):** C's heat panel and mine were both on
screen (and both on **Z**). Now there is one: the kit's chase HUD (`HeatPanel` + pursuit status,
bounty, escape bar, police radio), fed by C's `police_status`, which carries the extra fields my
readout used. The repair bill is priced live by G's `Economy::quote_repair` (`repair_quote_json`,
unchanged) and opens on its own the frame a chase ends — `Pursuit`/`Searching` → `Clear`/`Wanted`
(escaped) or a total loss mid-chase (wrecked) — with the wanted-level markup defaulted to the
current level. `apps/ui-kit/src/live/chaseEnd.ts` holds the detector (3 vitest cases).
**For E:** busted is not detectable yet — `PursuitStatus` has no `Busted`/arrest state and no
police event for it reaches the client; when E adds one, `ChaseEndDetector` should report
`"busted"` and the bill should show impound instead.

## Perf numbers

| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| HUD frame pacing, mock playing (3 s) | 4-core cloud container, headless Chromium 141 | — | 181 frames, p50 16.7 ms, p95 16.7 ms, 0 long tasks | 60 fps |
| Captain map (canvas redraw/frame, heat map 4 Hz) | same | — | 181 frames, p50 16.7 ms, p95 16.8 ms, 0 long tasks | 60 fps |
| Captain map on the recorded wp_police run (12 units, CaptainView interpolation) | same | — | 60 fps, 0 long tasks (e2e screenshot run) | 60 fps |
| Demo bundle (incl. fixtures + data) | — | — | 1.42 MB JS / 256 kB gzip (incl. the 410 kB `captain.jsonl` fixture), 30 kB CSS | — |

## Known gaps

- Race progress, crews, lobby and results are mock-only (see Needs). Police has a recorded real
  run (`police_pursuit`, E's city, not D's: E has not switched to D's road network yet).
- In recorded runs the captain's orders are validated with E's rules and shown as pending, but the
  recording cannot react to them. The toy police (other runs) does react, using E's rules.
- Real-run region damage is inferred from contact geometry until B emits `RegionDamaged`.
- Garage/mod numbers are TS ports of `wp_economy` (exact formulas, same tables); drift possible
  until UI-5.
- Gamepad/wheel binding capture uses the Gamepad API and is not exercised in CI (no device).
- Not mounted in the testbed yet (04's call); the demo page is the integration surface.

## Lead feedback
(Written by the integration lead: failing tests, expected vs actual.)

## Lead feedback (round 6, 2026-10-19)

**Done (M, 2026-10-19):** consolidated with C into one readout (kit components, `src/ui/`), bill
wired to G's `quote_repair` and opened at the end of a chase. Details under "Changes outside my
crates" → "Round 6".

Checks after merging the lead branch: fmt, clippy `-D warnings`, wasm32 build, testbed `tsc`, ui-kit
`tsc` + vitest (50 tests, incl. 3 for `ChaseEndDetector`), testbed e2e all pass (release wasm; with a `--dev` wasm the city-chase
check can time out before speeding reaches the feed). `cargo test --workspace --no-fail-fast`:
95 suites pass, **2 fail on the lead branch as merged, in code this branch does not change** —
`wp_economy/tests/pursuit_wiring.rs:130` (`real_police_capture_settles_through_the_economy`:
"level at capture, not after the reset") and `wp_race/tests/simcli_races.rs:28`
(`city_sprint_on_real_roads`: index out of bounds, len 2 index 2). For G and the race owner.


**Merged** (UI-kit glue in `apps/testbed-web/src/ui/`).

C also built a heat panel (`src/police_hud.tsx`); both are in `main.ts` now. **Consolidate with C**: one heat / wanted
/ pursuit readout using your components, and wire your repair-bill screen to G's economy (`quote_repair`) for the
demo's end-of-chase moment. Keep your code in `src/ui/`.


## Lead feedback (round 8, 2026-10-19)

Contract **v0.3.0 is on `main`** (typed police/race/economy events, damage snapshot/summary, road-network trait, weather `LocalConditions`; see `docs/CONTRACT_CHANGELOG.md`). Workspace tests now take about 3.5 minutes (`lead/test-speed`). Owner playtest items are tracked in `docs/PLAYTEST_LOG.md`.

1. **HUD consolidation clash.** You and C both consolidated the heat panel in round 6. On merge, the lead kept **C's** `apps/testbed-web` and `crates/wp_wasm/src/police.rs` (pursuit banner, capture opens the bill, `last_outcome`/`outcome_age_s` in `police_status`). Your ui-kit changes are in, but your testbed edits (`src/ui/overlay.ts` ChaseEndDetector wiring, `src/ui/police.ts`, `police_status` with `search_s`/`phase`) are not. Port the parts that add value on top of C's version (for example ChaseEndDetector, phase-scaled escape), coordinating via `docs/integration/testbed.md`. Keep exactly one chase panel.
2. Regenerate `src/generated/contracts.ts` and the recorded fixtures for v0.3.0's typed events (`npm run gen:types`, `gen:police`, `gen:fixtures`), and switch the reducers from payload bytes to typed variants.
