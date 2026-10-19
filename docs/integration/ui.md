# UI kit integration notes

- **Workstream / prompt:** 16 — UI kit (HUD, garage, mod tree, captain map, menus). Machine M.
- **Branch:** `ws/ui`
- **Crates / folders owned:** `apps/ui-kit` (new; TypeScript + Preact, not a Cargo workspace member)
- **Contract version built against:** 0.1.0 (codes toward the v0.2.0 plan where noted)

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
| HUD | damage silhouette by `BodyRegion` | `wp_damage` → `RegionDamaged` | v0.2 (B #3) + UI-7 | inferred from `Contact` geometry |
| HUD | detached parts by region | `wp_damage` → `PartDetached` | live, but group ids not mappable → UI-7 | guess (1 front, 2 rear) |
| HUD | glass | `wp_damage` → `GlassShattered` | v0.2 (B #1) | synthetic run |
| HUD | heat, wanted level, pursuit/cooldown, units chasing | `wp_police` → typed `PoliceEvent` | v0.2 (typed PoliceEvent) + UI-1 | synthetic run |
| HUD | bounty | `wp_economy` `PlayerAccount.bounty` + `BountyGained` | v0.2 (typed EconomyEvent) | mock account + event |
| HUD | minimap roads | `wp_world` `RoadNetwork` (`data/city/*.road.json`) | live (data) | derived fixture |
| HUD | minimap blips (racers, police, heli) | snapshot vehicles + UI-3 police units | live (vehicles) / req | metrics + toy police |
| HUD | race position, lap, checkpoint, gaps | race events (15) | req UI-6 | derived from distance |
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
| Results | classification, times | race events (15) | req UI-6 | mock |
| Results | payout lines | typed `EconomyEvent` (`RacePurse`, `CleanRaceBonus`, `RaceRep`, `RaceEntryFee`, `BountyGained`) | v0.2 (08 #1) | mock |
| Impound | cars in pound, fee/day, auction countdown, last capture | `wp_economy` capture / impound | live in crate → UI-5 | TS port of fees |
| Crews map | crews, district ownership/control | crews (15), `DistrictId` (05, deferred 08 #6) | req UI-10 | mock over real zones |
| Police career | rank, XP, unlocks, stats | `wp_economy` cop XP / `cop_level`, unlocks (06) | live (XP) / mock (unlocks) | mock |
| Police career | orders feed, assignment | captain commands (06/07) | req UI-1/UI-2 | events + mock |
| Captain map | roads, zones, POIs | `wp_world` | live (data) | derived fixture |
| Captain map | units (kind, status, pos, heading, target, human) | `wp_police` → snapshot (captain role) | req UI-3 | toy police sim |
| Captain map | suspects with last-known position / visibility | `wp_police` LOS + interest management (07) | req UI-3 | metrics + events |
| Captain map | incidents | `TrafficViolation` (+`pos`, v0.2 D #2), `Contact`, `Spotted` | live / v0.2 | events |
| Captain map | heat map | `wp_police` | req UI-4 (optional) | derived from incidents |
| Captain map | commands | `ClientToServer::CaptainCommand` | req UI-2 | mock |
| Scanner | dispatch/unit radio, violations, system lines | typed `PoliceEvent` (+ `Radio`), `TrafficViolation`, damage events | v0.2 + UI-1 | events |
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
| `captain` | `ClientToServer::CaptainCommand` | req UI-2 |
| `replay.*` | testbed replay player | 04 |

## Contract change requests

All of these are proposals: the shapes the UI is coded against live in
`apps/ui-kit/src/model/proposed.ts` and are deleted when the contract lands.

| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| UI-1 | Typed `PoliceEvent` variants the UI consumes: `HeatChanged { racer, heat: f32 0..1 within level, level: WantedLevel }`, `Spotted { racer, unit, pos }`, `PursuitStarted { racer, unit }`, `LostVisual { racer, unit }`, `CooldownStarted { racer, seconds }`, `Escaped { racer }`, `Busted { racer, unit }`, `UnitDispatched { unit, kind: PoliceUnitKind, target }`, `UnitStatus { unit, status, pos, heading_rad }`, `ToolDeployed { unit, tool, pos }`, `Radio { unit: Option<EntityId>, text }` | HUD heat meter, scanner feed, captain incidents, police orders. Input to the v0.2.0 "typed PoliceEvent" item that 06 designs; mock kind codes 1–11 in `POLICE_KIND`. Until then the mock decodes `payload` as UTF-8 JSON (same convention as `wp_economy`). | 06, 10, 08, 07 | open (for 06 / lead) |
| UI-2 | Typed `CaptainCommand` replacing `ClientToServer::CaptainCommand(Vec<u8>)`: `Assign { unit, target: Pursue { racer } \| Patrol { center: [f32;2], radius_m } \| Hold { pos } \| Incident { id } }`, `Deploy { tool: PoliceTool, pos, heading_rad }`, `Recall { unit }`, `SetPriority { racer, priority: u8 }`; `PoliceTool = SpikeStrip \| Roadblock \| Helicopter \| Spotlight \| Pit` | Captain map radial menu + drag-assign. Joins E's "captain command payload" item in the v0.2.0 batch. | 06, 07 | open |
| UI-3 | Captain-role snapshot data: `PoliceUnitSnapshot { entity, kind, status, pos, heading_rad, health, target: Option<…>, human: bool }` list and `SuspectSighting { racer, last_known_pos, visible: bool, at_tick }` in `ServerToClient::Snapshot` (captain only; interest management decides who gets it) | Captain map units / fog of war. Vehicles alone don't say which are police, what they are doing, or what the captain is allowed to know. | 06, 07 | open |
| UI-4 | (optional) Coarse heat field for the captain: `Vec<HeatSample { pos: [f32;2], weight: f32 }>` or a small grid, at ≤ 1 Hz | Heat-map layer. The UI can derive it from incidents if 06 prefers not to send it. | 06, 07 | open, low priority |
| UI-5 | (wasm surface, not contract) `wp_wasm` exports for the garage: `economy_preview(def_id, mods, mode) → Preview`, `quote_repair(account, car, option, shop, mode)`, `current_value`, `sale_price`, `retrieval_fee`, `market_price` (JSON in/out) | Lets the UI drop its ports of `estimate.rs` and the billing rules (risk of drift). | 04, 08 | open (for 04 / 08) |
| UI-6 | Race progress: `RaceProgress { racer, position, lap, checkpoint, gap_ahead_s: Option<f32>, gap_behind_s: Option<f32>, finished_s: Option<f32>, status }` per racer in the snapshot, plus `SimEvent::CheckpointPassed { racer, index, lap }` | HUD race panel, results classification. | 15, 07 | open (for 15) |
| UI-7 | Refinements to the accepted damage events: `RegionDamaged.severity` is **cumulative** 0..1 (not a delta), emitted when it changes by ≥ 0.05; `PartDetached` gains `region: BodyRegion` | HUD can set instead of accumulate (and stays right after a late join); group ids are lattice-specific, so the UI cannot map them to the silhouette. | 03, 07 | open |
| UI-8 | Typed `EconomyEvent` (already accepted, 08 #1): keep `EventReason` variant names stable and include `RaceEntryFee`, `RacePurse`, `CleanRaceBonus`, `RaceRep`, `BountyGained` as today | Results payout lines map 1:1 to reasons. | 08 | note on an accepted item |
| UI-9 | `ServerToClient::Snapshot` gains `time_of_day: TimeOfDay` (or a separate low-rate message) | HUD clock, night-readability decisions. | 05, 07 | open |
| UI-10 | `DistrictId` (deferred 08 #6 → 05) with display names, and a crew/territory state owned by 15: `DistrictControl { district, crew: Option<CrewId>, control: f32, contested: bool }` | Crews & territory map (today: zone index + mock owners). | 05, 15, 08 | open |

## Changes outside my crates

None. `apps/ui-kit` is new. It reads (never writes) `data/`, `schemas/`, `crates/wp_contracts`
(via the standalone schema tool, which has its own `[workspace]` so the root workspace is
untouched) and runs `simcli` to produce fixtures.

## Perf numbers

| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| HUD frame pacing, mock playing (3 s) | 4-core cloud container, headless Chromium 141 | — | 181 frames, p50 16.7 ms, p95 16.7 ms, 0 long tasks | 60 fps |
| Captain map (canvas redraw/frame, heat map 4 Hz) | same | — | 181 frames, p50 16.7 ms, p95 16.8 ms, 0 long tasks | 60 fps |
| Demo bundle (incl. fixtures + data) | — | — | 618 kB JS (145 kB gz), 30 kB CSS | — |

## Known gaps

- Police, race progress, crews, lobby and results data are mock-only (see Needs).
- Real-run region damage is inferred from contact geometry until `RegionDamaged` is emitted.
- Garage/mod numbers are TS ports of `wp_economy` (exact formulas, same tables); drift possible
  until UI-5.
- Gamepad/wheel binding capture uses the Gamepad API and is not exercised in CI (no device).
- Not mounted in the testbed yet (04's call); the demo page is the integration surface.

## Lead feedback
(Written by the integration lead: failing tests, expected vs actual.)
