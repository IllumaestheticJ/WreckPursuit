# UI kit integration notes

- **Workstream / prompt:** 16 — UI kit (HUD, garage, mod tree, captain map, menus). Machine M.
- **Branch:** `ws/ui`
- **Crates / folders owned:** `apps/ui-kit` (new; TypeScript + Preact, not a Cargo workspace member)
- **Contract version built against:** 0.3.0 (merged `main` after lead round 8). Police, economy
  and race events and captain commands are the generated contract types.

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
| `model/police.ts` | re-exports the generated `PoliceEvent`, `CaptainCommand`, `UnitOrder`, `CaptainError`, …; hand mirror of `wp_police::{CaptainView, UnitView, RacerSighting, ToolReadiness, Incident}` (not in the contract, UI-3); `decodePoliceEvent`, `captainMessage` (→ `ClientToServer::CaptainCommand`), `captainErrorText` | client code talking to police |
| City map HUD (`src/map/`, exported from `src/live`) | `mountMapHud(parent, { onOpenChange?, minimapPlacement? }) → MapHud` with `setStatic(json)` once per city, `frame(map_dynamic, nowMs)` each frame, `toggle()` / `open()` / `close()` / `isOpen`, `setSignals`, `setMinimapVisible`, `stats()`; pure helpers `decodeMapDynamic`, `worldToMinimap`, `minimapMatrix`, `radiusForSpeed`, `fitBounds`, `zoomAt`, `pan`, `districtLabels` | testbed (mounted), later the game client |
| Game menu (`src/menu/`, exported from `src/live`) | `mountGameMenu(parent, { host, settings, cameras?, countersteer?, developerDefault? }) → GameMenu` with `openMain()` / `openPause()` / `openSettings()` / `close()`, `screen`, `isOpen`, `settings`, `update(patch)`, `setCountersteerAvailable(on)`; host: `play(choice)`, `restart`, `garage`, `quitToMain`, `openControls`, `controlsOpen`, `apply(next, prev)`, `onOpenChange(open, screen)`; settings `GameSettings` (`defaultGameSettings`, `loadGameSettings`, `saveGameSettings`, `normalizeSettings`; `localStorage` `wp.gameSettings.v1`) | testbed (mounted), the demo build, later the game client |
| `ChaseEndDetector` (`src/live/chaseEnd.ts`) | `update(racer, status, level, events, freshOutcome?) → { how, outcome, level } \| null`, once per chase | testbed (bill on capture / wreck), any live host |
| Demo | `npm run demo` → http://localhost:5180 | everyone |

Wiring into the testbed (04 owns it; nothing edited there):

```ts
import { mountUi, createMockModel } from "../../ui-kit/src";
const hud = mountUi(document.getElementById("hud-overlay")!, { model: createMockModel({ screen: "hud" }), screen: "hud", globalKeys: false });
```

## City map HUD (round 9, owner request "we need a game map HUD with police")

Screenshots: `docs/media/map/` (`node apps/testbed-web/e2e/map_shots.mjs`).

- **Minimap** (always on; testbed layer `'` "minimap", bottom-right corner since round 12; see
  "Round 12" for the HUD layout): heading up,
  radius 160 m standing → 480 m at 45 m/s (smoothed), roads blitted from one offscreen canvas per
  city (0.76 px/m for Port Vesper, ≈ 2600 × 2400 px), bridge decks drawn over a dark casing so
  Loop 9's overpass reads as on top. Police: lights on (respond / pursue / intercept / roadblock /
  spikes) = alternating red/blue dot with a halo; patrol / idle = steady dim blue; helicopter =
  diamond with a rotor ring. Units outside the disc that are chasing you (or the helicopter) sit
  on the rim. While wanted: every ground unit's sight radius (one union fill; the police see all
  round, buildings permitting, so this is a circle, not a cone), the helicopter's range ring and
  spotlight disc, intercepting units' target point; while SEARCHING: the search zone (last known
  position, dashed amber) and the escape ring around the rim with `ESCAPE nn%`. Roadblocks
  (red/white bar) and spike strips (yellow teeth). Garages and safehouses as icons. Traffic
  signals optional (testbed layer `;`). Rim: red/blue blinking in pursuit, amber searching.
  Caption: district + speed.
- **Full map** (testbed **M** or **Tab**; **M** / **Tab** / **Esc** close): whole city fitted,
  districts tinted by kind and named, roads by class, decks cased, POIs with names (garages,
  safehouses, police stations, helipads, race starts), player with a locator ring, police and
  sight / search as on the minimap. Drag / arrows / WASD pan, wheel / + / − zoom at the cursor,
  0 whole city, F follow. Legend panel (swatches drawn by the same code as the map). While open,
  the testbed sends the player's car neutral input with brake 0.35 (input paused, sim running).
- **District names:** the city data has no zone names (CITY.md C2, still open), so Port Vesper
  v2's names live in `src/map/districts.ts` by zone index (Lantern Row, Midtown, Harbor Blvd,
  Gantry Row, Canal Flats, Mercer, Eastgate, Hollins Heights, Bayview Bluffs, Larkspur); other
  cities show the district kind. A ring zone (Midtown around Lantern Row) is labelled on the ring.
- **Style:** scanner-green monospace for titles / labels / player; roads muted grey-green so the
  police lights are the brightest thing; grime + scanlines on the full-map chrome only (off with
  the high-contrast palette); colours come from the kit's CSS variables, so the colour-blind
  palettes apply.
- **Search radius** is presentation only: `wp_police` has no explicit search area, so the zone is
  `min(60 m + 10 m/s × time since line of sight was lost, 600 m)` around `last_known`
  (`wp_wasm::map::search_radius`). **For E:** if police get a real search area, put it in the
  record and I'll draw that instead.

### wasm surface (`crates/wp_wasm/src/map.rs`, C's crate; minimal, read-only)

| Function | Returns |
|---|---|
| `map_static_json()` | `null` on the flat world, else `{ city, bounds: [x0,y0,x1,y1], roads: [{ class 0 highway…4 alley, layer 0 grade / 1 deck, pts: [x,y,…] }], zones: [{ id, kind, poly }], pois: [{ kind, name, pos }], signals: [[x,y]] }` from `RoadGraphView` (classes) + each road's own `RoadSurface` vs the terrain (deck = > 1.5 m above, as `deck_rails`). Port Vesper: 103 kB JSON. |
| `map_dynamic(player)` | `f32` array: header 21 (`[version 2, n_units, n_tools, status 0 clear/1 wanted/2 pursuit/3 searching, level, search_x, search_y, search_r, escape 0..1, car_sight_m, heli_sight_m, heli_spot_m, last_x, last_y, has_last, player_x, player_y, player_yaw, player_speed, has_player, n_racers]`), then per unit 8 (`[id, x, y, yaw, speed, flags, target_x, target_y]`; flags = state code 0–8 in bits 0–3 + 16 heli, 32 lights, 64 on player, 128 wrecked, 256 human, 512 has target), then per tool 6 (`[kind 0 roadblock / 1 spikes, x, y, dir_x, dir_y, half_len_m]`), then (v2, owner row 100) per racer 6 in race-position order (`[entity, x, y, yaw, position, flags]`; flags 1 finished, 2 DNF, 4 captured (with DNF), 8 DQ, 16 the player's own car; `n_racers` 0 outside a race). The map draws racers still in the race (not the player, not finished / DNF / DQ) as violet arrows (`--racer`) with a position tag; the leader larger with a light ring. Every unit, bound or internal, from `wp_police` unit bodies. `police_units()` / `police_status()` stay as they are (C's 3D view and the chase readout use them). |

Tests: `wp_wasm` `map::tests` (Vesper: classes, decks, zones, POIs, size; grid flat; dynamic
layout vs the police unit count and the player pose/yaw convention; search radius); ui-kit
`src/map/*.test.ts` (23: heading-up projection and rotation for any yaw, inverse, canvas matrix,
composition with the road cache, glyph rotation, speed zoom, rim clamp, full-map fit / zoom at
cursor / pan, decoder, district labels); testbed e2e (minimap shows the player + police dots in
the city chase; M opens, M closes, Tab opens, Esc closes).

## Round 12 — HUD layout + game menu (owner requests 2026-10-20, playtest rows 67, 68)

Base: `origin/lead/round12` merged into `ws/ui`.

**Row 67 — no HUD overlaps.** The minimap moved to the **bottom-right** corner (was left column,
top 312 px, on top of the input panel and the developer panel). Corners now: top-left speedo
(+ input panel) with the race HUD stacked under it; top-centre chase readout, the weather line
under it, pursuit banner; top-right damage silhouette + components / regions / event feed,
capped to end above the minimap; bottom-right minimap; bottom-left developer panel with the audio
panel stacked above it (it used to sit bottom-right); telemetry between them. The sizes that change
at run time (race HUD appears, panels fold, the feed grows, the window resizes) are handled by a
small layout pass, `apps/testbed-web/src/ui/layout.ts` (≤ 10 Hz, writes `top` / `max-height` /
`left` only). The developer panel and audio panel shrink and scroll when a race HUD needs the
room at 720p. Verified by bounding boxes of every HUD element (`e2e/hud_layout.mjs`): 0 overlaps
at 1280×720, 1366×768 and 1920×1080, developer panels on and off, in a rainy city chase (banner,
weather line and a full event feed forced on) and in a street race.

**Row 68 — game menu** (`apps/ui-kit/src/menu/`, Preact, kit theme):
- **Pause** (Esc or gamepad Start / the rebindable "pause" action, P by default; pauses the sim.
  Not in replays: there P / Menu stay play/pause): Resume, Restart, Garage (career),
  Settings, Controls, Quit to main menu. Esc still closes the city map, the controls panel and
  the garage first (the menu only opens when nothing else is on screen).
- **Main menu** (like the demo's start page: City chase, Rainy night, Street races, Free drive,
  Garage, Settings, Controls): shown on load with `?menu=1` or on the demo page (`body.demo`); the
  testbed then removes the demo's own `#demo-menu` / `#demo-open`. Quick play runs outside the
  career, as the demo did; Free drive is the city with traffic.
- **Settings** (persist in `localStorage` `wp.gameSettings.v1`, try/catch, applied live):
  quality auto / high / low (`Quality.setMode`: resolution scale, governor, rain density, panel
  blur and draw distance at once; MSAA after a reload) + draw distance (from quality / 300 m …
  3000 m); volumes master / engine / effects (World bus) / police radio (Police bus) / ambience
  (`AudioSystem.setMix`); assists ABS / TCS / ESC / auto gears / steering help (through the same
  `wp.assistOverrides` as the developer panel's boxes; steering help off zeroes
  `steer_speed_sensitivity`) and **countersteer 0–100 %, disabled until machine A's wasm setter
  lands** (the testbed enables it when `wp_wasm` exports a function whose name contains
  `countersteer`, called as `(entity, strength 0..1)` after every assist preset — **A: please
  confirm or tell me the name / signature**); damage mode Arcade / Simulation (live switch, not
  during a career run); km/h or mph (speedo + minimap caption); default camera; developer panels.
  First run seeds the settings from what the browser already chose (assist boxes, audio panel,
  `wp.gfx`).
- **Controls** opens C's rebinding panel (`src/ui/controls.ts`, reused as is — not forked, not
  edited); the menu hides while it is open and comes back when it closes.
- **Developer toggle** (Settings → Developer): on by default in the testbed preview, off with
  `?menu=1` and in the demo build; `?dev=0|1` forces it for one page. Off = `body.wp-player`
  hides the scenario/replay panel, telemetry, components, regions, event feed, input display,
  audio mixer and the perf lines of the speedo, and suppresses the 3D debug layers (tyre force
  arrows, slip colours, CG / velocity, contacts, lattice, intents).
- Keyboard + gamepad navigable (↑↓ / W S / Tab, ← → change, Enter / Space, Esc / Backspace;
  d-pad / left stick, A, B, Start). Unit tests `src/menu/menu.test.ts` (7).
- Style: low-poly dark per `docs/art/STYLE_DECISION.md` with the kit's theme variables; machine S's
  `render/palette.ts` is not on this branch yet — when it lands, its UI tokens map onto the theme
  variables (one place).

Screenshots (headless, `MENU_SHOTS=dir npm run e2e:menu`): main menu, pause, settings and every
layout case.

**e2e** `apps/testbed-web/e2e/menu.mjs` (`npm run e2e:menu`; also run at the end of `npm run e2e`),
36 checks: 12 layout cases (3 sizes × dev on/off × chase/race) with 0 overlaps; Esc pauses (tick
frozen) and Esc resumes; a game key pressed in the menu does not reach the game; ↑ / ↓ / Enter /
→ / ← navigation into settings; units and master volume change and reach the HUD / audio mix; a
setting survives a reload; countersteer slider disabled; gamepad Start opens, d-pad moves, B
resumes; Controls opens C's panel and returns; `?menu=1` title screen with dev panels off, Enter
starts the city chase; no page errors.

**Checks (round 12, this container, `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
CARGO_PROFILE_TEST_DEBUG=0`):** `cargo fmt --check` ✓, `cargo clippy --workspace -D warnings` ✓,
wasm32 release build of `wp_wasm` ✓ (packaged with `wasm-bindgen` 0.2.129; no `wasm-pack` here,
so `npm run e2e` was run directly instead of `just web-e2e`), testbed `tsc` ✓, ui-kit `tsc` +
vitest 88 / 88 ✓, ui-kit e2e ✓, testbed `npm run e2e`: 126 PASS incl. all 36 menu checks;
**fails only `perf_check`** (chase / rain real-time factor 0.25, 1.32 M / 1.25 M triangles per
frame vs the 700 k limit). **Same numbers on the round-12 base** without this branch
(`frame_profile.mjs chase 12`: base 1,318,895 triangles, RTF 0.270; this branch 1,318,895, RTF
0.269), so it comes from round 12's merges (parked cars / props or LOD?), not the menu.
`cargo test --workspace --no-fail-fast`: 141 suites, 952 pass, **4 fail on the base** (this branch
changes no Rust, data or scenario file): simcli `goldens_match`, wp_race `grid_highway_sprint`,
wp_sim `vesper_v2_cars_drive_up_the_kerb_and_along_the_sidewalk`, wp_wasm
`race_tests::short_sprint_with_ai_runs_to_results_and_pays_through_the_economy` (player DNF
`FinishWindow`). **For the lead.**

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
| UI-8 | Typed `EconomyEvent` (v0.3.0, G #1): keep `EventReason` names stable incl. `RaceEntryFee`, `RacePurse`, `CleanRaceBonus`, `RaceRep`, `BountyGained` | Results payout lines map 1:1 to reasons. | 08 | **done in v0.3.0** (`StakeCharged` / `RacePayout` renames taken; the UI reads `SimEvent::Economy`) |
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
**C and the lead, please review**. (Round 8: the lead kept C's testbed and `wp_wasm/src/police.rs`;
my round-6 `src/ui/police.ts` and the extra `police_status` fields are not on `main` and their rows
are removed. C's `src/police.ts` maps the status onto the kit's readout.)

| File | Owner | Change |
|---|---|---|
| `apps/testbed-web/src/main.ts` (round 12) | C | Mount the kit's game menu (host callbacks, settings applied live, Esc capture listener, "pause" action opens the pause menu outside replays, `?menu=1` / `?dev=`, demo start page replaced); `HudLayout.frame()` each frame; `steer_help` override; HUD units; `menu`, `layout` on `window.__wp`. |
| `apps/testbed-web/src/ui/layout.ts` (new, round 12) | C | HUD layout pass (row 67). |
| `apps/testbed-web/src/ui/map.ts` (round 12) | C | Minimap bottom-right. |
| `apps/testbed-web/index.html` (round 12) | C | Right column shrinks to fit; `body.wp-player` hides the developer panels. |
| `apps/testbed-web/src/settings.ts` (round 12) | C | `layers.suppress(ids)` (player mode forces debug layers off; toggles keep their state). |
| `apps/testbed-web/src/hud.ts` (round 12) | C | `HudFrame.units` (km/h / mph); speedo never shows "-0". |
| `apps/testbed-web/src/render/quality.ts` (round 12) | lead (browser perf) | `Quality.setMode(mode)`: live switch (stored); `mode`, `drawDistanceM`, `rainFraction` no longer readonly. |
| `apps/testbed-web/src/audio/index.ts` (round 12) | J / C | `AudioSystem.setMix(volumes)` / `mix()` (master + Engine / World / Police buses + ambience, saved with the audio panel's settings). |
| `apps/testbed-web/e2e/menu.mjs`, `e2e/hud_layout.mjs` (new), `e2e/run.mjs`, `package.json` (round 12) | C | Menu + layout e2e, run at the end of `npm run e2e`; `e2e:menu` script. |
| `crates/wp_wasm/src/garage.rs` (new) | C | `repair_quote_json(entity, request_json)`: the running car's `DamageSim::repair_estimate` (economy `PriceTable` for its class/value) priced by `wp_economy::Economy::quote_repair` (shop, district, mode, wanted markup, insurance, salvage). Read-only. |
| `crates/wp_wasm/src/lib.rs` | C | `pub mod garage;` |
| `crates/wp_wasm/Cargo.toml` | C | `wp_economy` dependency (default features; `cargo tree --target wasm32-unknown-unknown` shows no rusqlite). |
| `crates/wp_wasm/src/map.rs` (new), `lib.rs` (`pub mod map`, `map_static_json`, `map_dynamic`) | C | City map HUD data (round 9), read-only. |
| `apps/testbed-web/src/ui/map.ts` (new), `main.ts`, `wasm/sim.ts`, `settings.ts` | C | Mount the kit's map HUD; `map.reset()` on rebuild, `map.frame()` after `ui.frame`; **M** / **Tab** toggle the full map; neutral input + brake 0.35 while it is open; layers `'` minimap, `;` minimap signals; `map` on `window.__wp`. |
| `apps/testbed-web/src/audio/index.ts`, `audio/panel.ts`, `hud.ts` | C / J | **Mute moved M → V; "follow the next car" moved Tab → backquote** (the owner asked for M / Tab on the map); help text updated. |
| `apps/testbed-web/e2e/run.mjs`, `e2e/map_shots.mjs` (new) | C | Map checks after the city chase; screenshot script for `docs/media/map/`. |
| `apps/testbed-web/src/ui/overlay.ts` (round 8) | C | Chase end via the kit's `ChaseEndDetector` (typed `PursuitEnded`, else status + fresh `last_outcome`) instead of back-computing the end tick from `outcome_age_s`; bill still opens on capture / wreck only (C's choice: not on escape); `ui.lastChaseEnd` = `{ how, outcome, level, tick }`. |
| `apps/testbed-web/e2e/run.mjs` (round 8) | C | Stubbed banner flow: a pursuit frame before the capture (a capture always follows a chase), and checks `ui.lastChaseEnd` (escaped, then captured at L3). |
| `apps/testbed-web/src/ui/overlay.ts` (new; all testbed-side UI-kit glue lives in `src/ui/`) | C | Mounts the kit's `chaseHud` and `repairBill` overlays on a `createLiveModel()` fed each frame with the drained events + `police_status`. Opens the bill by itself when a chase ends (kit's `ChaseEndDetector`); `ui.lastChaseEnd` records how. |
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
| City map HUD, Port Vesper city chase (5 units), testbed, sim paused (`map_shots`-style script) | same | — | `map_static_json` 4.9 ms + parse 1.7 ms + road cache 24 ms, once per city; `map_dynamic` 0.007 ms; minimap draw 0.07 ms/frame; full map open (+ minimap) 0.27 ms/frame (CPU time of the canvas calls; headless GPU raster not included) | 60 fps |
| Demo bundle (incl. fixtures + data) | — | — | 1.42 MB JS / 256 kB gzip (incl. the 410 kB `captain.jsonl` fixture), 30 kB CSS | — |

## Checks (round 9, this container, after merging `main`)

`cargo fmt --check`, `cargo clippy --workspace -D warnings`, `cargo test --workspace` (120 suites,
0 failures), wasm32 release build of `wp_wasm` (packaged with `wasm-bindgen` 0.2.129; no
wasm-pack here), testbed `tsc`, ui-kit `tsc` + vitest 81 / 81, ui-kit e2e (60 fps, 0 long tasks),
testbed `npm run e2e`: all checks pass, including the two new map checks. The first e2e run of
the session failed both `debugParkNearPolice`-based checks (the map one and the lead's capture
check: the park never succeeded within its timeout) and logged an audio
`setValueCurveAtTime overlaps` page error; the rerun passed everything. Looks like a flake in the
park helper / audio scheduling under load, not in the map code (it only reads); **for C / J** if
it shows up again.

## Known gaps

- Map: zone names are a UI-side table for Port Vesper v2 (CITY.md C2 asks for `Zone.name`); the
  search zone radius is UI-defined (no police search area); the minimap's signals are junction
  markers, not live colours; the kit's demo page has no map screen (the testbed is its surface);
  no water polygons in the city data, so the sea is just background.

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

**Done (M, round 8, 2026-10-19):**

1. **HUD clash.** Took C's testbed as kept by the lead (one chase panel: the kit's chase HUD + C's
   pursuit banner). Of my round-6 testbed edits, `src/ui/police.ts` and the `search_s` / `phase`
   fields are **dropped**: C's `police_status` already has phase-scaled `escape_s` and
   `staleness_s`, so they added nothing. Ported on top of C: the **`ChaseEndDetector`**, now
   reading the typed `PoliceEvent::PursuitEnded` (outcome + level before the reset) and falling
   back to the status turning clear with C's fresh `last_outcome`; it replaces C's
   `outcome_age_s` → end-tick arithmetic and also opens the bill on a total loss mid-chase.
   C's UX kept: escapes show the banner but don't pop the bill. Listed under "Changes outside my
   crates"; note for C in `docs/integration/testbed.md`.
2. **v0.3.0.** `npm run gen:types` (now also emits `ClientToServer`), `gen:police`,
   `gen:fixtures`, `gen-synthetic` re-run. Reducers match `SimEvent::Police` /
   `SimEvent::Economy` directly; the payload decoders and `EconomyEventMirror` are gone;
   `captainCommandBytes` → `captainMessage` (typed `ClientToServer::CaptainCommand`);
   `RequestRoadblock` / `RequestSpikes` use `road` (was `segment`). vitest 58 / 58.
   - **For E:** the re-recorded `police_pursuit` (same seed and script) now has 0 `SpikeHit`
     (was 3) and the pursuit is still active at 240 s; radio lines are batched ("Units 7, 8 and 9
     out of service."). The recorded-run test now checks spikes are *placed*; the hit line is unit
     tested.

3. **Checks (this container, after merging `main`):** `cargo fmt --check`, `cargo clippy
   --workspace -D warnings`, `cargo test --workspace` (115 suites, 0 failures), wasm32 build of
   `wp_wasm`, testbed `tsc`, ui-kit `tsc` + vitest 58 / 58, ui-kit e2e (60 fps, 0 long tasks), and
   testbed `npm run e2e` (release wasm) — all pass, including "pursuit banner: PURSUIT → SEARCHING
   → ESCAPED (escaped) → CAPTURED (bill opened, chase end captured at L3)". The reworked golden
   frontal scenario (4 s, parked car) changed the crash fixtures; the fixture tests now assert
   ranges for it (rear bumper < half the front, heading checked before the hit).

## Lead helper note (2026-10-20) — car stat bars in the garage and market

`data/stats/cars.json` (physics-measured by `simcli car-stats`, see `docs/integration/testbed.md`)
is read at build time like the car files: `CAR_STATS` in `src/data/catalog.ts`. New kit component
`CarStatBars` (`src/ui/kit.tsx`, segmented `Meter`s; `compact` = unlabelled bars, numbers in the
tooltip). Garage: the selected car's panel shows acceleration / top speed / cornering with values
(labelled "stock figures" when mods are installed; the mod tree keeps the estimator's build
numbers). Market: an "Accel · Top · Corner" column per listing. Test: every car in `CARS` has stats
with bars in 0–1. The kit e2e (`npm run e2e`) could not run in this container (Chromium crashed /
hung on `repairBill` with and without this change); garage and market were checked by hand in
headless Chromium (renders, no console errors).

### Follow-up (2026-10-20): bars reflect installed mods

`src/data/stats.ts` `buildStats(defId, mods, measured?)`: stock → the measured table; with mods →
`GarageCar.stats` when a provider measured the build (new optional field; live providers fill it
from `wp_wasm::car_stats_build_json`), else stock × the estimator's modded/stock ratio
(`previewBuild`), so stock cars still show exactly the measured numbers. Kestrel TS + cold air
intake + race ECU + semi-slicks: estimate 6.46 s / 255 km/h / 1.02 g, rig 6.54 s / 256 km/h /
1.05 g (test keeps the estimate within 8 %). `CarStatBars` takes a `baseline` (tick + better/worse
colour). Garage: build bars with a stock tick, labelled measured / estimated. Mod tree: the
candidate build's bars with a tick at the current build. The mod tree's delta table shows the
bar values for 0–100, top speed and cornering (50 m) so they match the bars; class index, power,
mass, power/weight and braking stay the economy estimator's (the class index is computed from the
estimator, as in `wp_economy`). Cars without stats fall back to the estimator rows.
