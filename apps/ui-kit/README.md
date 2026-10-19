# apps/ui-kit — WreckPursuit game UI kit

**Owned by workstream 16 (UI kit).** Branch `ws/ui`. Integration notes: `docs/integration/ui.md`.

Every screen the prototype needs, built against mock data shaped like the real contracts, so the
UI is done the moment the sim exposes the data:

| Group | Screens |
|---|---|
| Race | **HUD** (speed / rpm / gear, damage silhouette by `BodyRegion`, component list, heat & wanted meter, minimap, scanner alerts, race position / checkpoints / gaps, assist badges) · **Replay controls** |
| Menus | Main menu · Pause · **Settings** (full input remapping with keyboard / gamepad / wheel binding flow, assist matrix, damage mode, accessibility: colour-blind palettes, UI scale, reduce motion; rumble / FFB / audio) |
| Garage | **Garage** (car list, condition, repair / choose items / drive damaged / scrap with an itemised bill, insurance) · **Mod tree** (slots, lines, tiers, requirements, performance-vs-durability before/after, class index) · Market (buy / sell) |
| Street | Race lobby / meet (asymmetric roles) · Results + payout breakdown · Impound · Crews & territory map |
| Police | Police career (rank, unlocks, orders feed) · **Captain map** (2D canvas: roads, units, incidents, heat map, command radial, drag-to-assign) · Scanner feed |

Style: art direction option B ("stylized low-poly, dark"): near-black surfaces, scanner-green
monospace accents, grime + scanlines on menus, hard text shadows on the HUD for night readability.

## Run it

```sh
cd apps/ui-kit
npm install
npm run demo        # http://localhost:5180 — every screen, mock playback, a11y toggles
npm test            # vitest: parsing, event reduction, economy/estimator ports, mock provider (43 tests)
npm run typecheck
npm run e2e         # builds, serves, visits every screen in headless Chromium, screenshots → e2e/out/
```

Demo URL parameters: `?screen=garage&run=golden_tbone&t=5&palette=deuteranopia&scale=1.25&paused=1&chrome=0`.
Press <kbd>`</kbd> to hide/show the demo sidebar. `window.wpModel` is the live mock model.

## Why Preact

Preact (10.27, ~4 kB gz) with hooks and JSX: the smallest mainstream option that type-checks with
plain `tsc` (no compiler plugin, unlike Svelte), so it fits the testbed's existing TypeScript +
Vite setup with one `esbuild.jsx` line. No router, no state library: one observable store
(`Store`) and a `useSlice` hook. The only runtime dependency is `preact`.

## Mount API

```ts
import { mountUi, createMockModel } from "@wreckpursuit/ui-kit";

const model = createMockModel();                       // or the real provider, see GameModel
const ui = mountUi(document.getElementById("ui")!, { model });

// HUD only, as an overlay on the 3D canvas, input owned by the host:
const hud = mountUi(overlayEl, { model, screen: "hud", globalKeys: false });

hud.setScreen("pause");   // pin another screen (undefined = follow model state)
ui.unmount();
```

- `mountUi(el, { model, screen?, globalKeys? }) → { unmount, setScreen }`. The element must be
  positioned; the kit fills it with `position: absolute; inset: 0`. All CSS is scoped under
  `.wp-ui`, so it never restyles the host page.
- Individual pieces are exported for hosts that compose their own layout: `App`, `Hud`,
  `DamageSilhouette`, `Minimap`, `ReplayControls`, `CaptainMap`, `SCREENS`, plus `ModelContext`,
  `useSlice`, `useSend` for custom components.

### `GameModel` — the one interface

```ts
interface GameModel {
  get(): UiState;                                        // immutable snapshot
  subscribe(listener: (s: UiState) => void): () => void; // after every change
  send(command: UiCommand): void;                        // every user action
}
```

- `UiState` (`src/model/types.ts`) is one tree with a slice per screen (`hud`, `scanner`,
  `settings`, `account`, `garage`, `market`, `lobby`, `results`, `impound`, `crew`, `police`,
  `captain`, `map`, `replay`) plus `screen` and `notice` (toast for refused commands).
- `UiCommand` (`src/model/commands.ts`) is a discriminated union: `navigate`, `settings.bind`,
  `garage.repair`, `mods.install`, `market.buy`, `lobby.setRole`, `impound.retrieve`,
  `captain` (typed `CaptainCommand`), `replay.seek`, … The provider validates (the economy is
  server-authoritative) and either updates the state or sets `notice`.
- Providers replace changed objects and never mutate, so `useSlice(select)` re-renders a
  component only when its slice changes by reference.
- `docs/integration/ui.md` maps every slice and command to the crate that will own it.

A real provider (wp_wasm in the browser, wp_net on a client) implements the same three methods:
fold `SimEvent`s with the exported `applyEvent` reducer, copy `VehicleState` into `hud`, and turn
commands into economy calls / `ClientToServer` messages.

### Mock provider

`createMockModel({ run?, autoplay?, loop?, clock?, screen?, role? })` returns a `GameModel` plus
`advance(seconds)`, `dispose()` and `run()`.

- It **replays a simcli run**: `events.jsonl` (every `SimEvent`) + `metrics.csv` (speed, rpm,
  gear, health, position per entity) + `summary.json` (entity ids). The replay transport *is* the
  replay screen: seeking resets and re-folds, so it is deterministic. `clock: "manual"` (tests)
  only moves on `advance()`; `"raf"` follows `requestAnimationFrame`. No wall clock reaches the
  derived sim tick.
- Garage, market, mod tree, impound and lobby commands run against a local mock economy that
  mirrors `wp_economy`'s rules and numbers (`src/data/economy.ts`, `src/data/estimate.ts` — the
  estimator and class index are line-by-line ports of `crates/wp_economy/src/estimate.rs`).
- **Police:** a run recorded from E's real `wp_police` (`police_pursuit`) drives the captain map from
  its `CaptainView` samples (interpolated) and the HUD heat from the player's `RacerSighting`.
  Captain commands are validated with E's rules and shown as pending (a recording cannot react).
  Other runs use a toy police (`src/mock/police.ts`) with E's unit assignments and E's command
  rules (NotPursued, Locked, Cooldown, Budget, from `data/police/*.toml`), which does react.
- Car and mod data are the real `data/cars/*.json` and `data/mods/*.json`; maps are the real
  `data/city` road network.

### Fixtures (`fixtures/`)

| Run | Source | What it shows |
|---|---|---|
| `synthetic_pursuit` | `scripts/gen-synthetic.mjs` (hand-scripted, **not** sim output) | 90 s night race + pursuit: violations, L1→L5, PIT, spikes, roadblock, helicopter, escape, region damage, a bounty `EconomyEvent` (wp_police event shapes) |
| `police_pursuit` | `npm run gen:police` (`tools/record-police`: E's real `PoliceSystem` + a scripted racer) | 240 s pursuit in E's police city: real `PoliceEvent`s, `captain.jsonl` (`CaptainView` at 0.5 Hz), the city as `map.json` |
| `golden_frontal_56kmh` | `simcli run` | real frontal crash: contact, component damage, failed lights, detached parts |
| `golden_tbone` | `simcli run` | real side impact |
| `city/procedural_12x12.map.json` | derived from `data/city/procedural_12x12.road.json` | roads (one polyline per road), zones, POIs |

`npm run gen:fixtures` re-runs simcli and re-derives the map; `node scripts/gen-synthetic.mjs`
rewrites the synthetic run; `npm run gen:police` re-records the police run. A run directory may
carry `captain.jsonl` and `map.json` in addition to the simcli files. simcli itself emits no police
events yet (police is not wired into `wp_sim`), hence the recorder.

## Contract types

```sh
npm run gen:types                 # cargo → runtime schemas, then json-schema-to-typescript
npm run gen:types -- --skip-cargo # only the TypeScript step
```

1. `tools/gen-runtime-schemas` (a standalone Rust crate with its own `[workspace]`, so neither the
   root workspace nor `wp_contracts` is touched) writes JSON Schemas for the runtime types the UI
   reads (`SimEvent`, `VehicleState`, `VehicleInput`, `AssistProfile`, `HandlingEffects`,
   `RepairEstimate`, `Role`, `DamageMode`, `TimeOfDay`, `ServerToClient`, …) to `schemas/runtime/`.
2. `scripts/gen-types.mjs` merges those with the lead's data schemas (`../../schemas/*.json`),
   normalises draft 2020-12 tuples for json-schema-to-typescript, and writes one
   `src/generated/contracts.ts` (committed, so consumers do not need cargo).

Police types are a hand mirror of `wp_police` in `src/model/police.ts` (the contract carries them
as opaque JSON payloads until v0.3.0); `src/model/proposed.ts` keeps only the economy event mirror.
Both are replaced by generated types when v0.3.0 lands.

## Accessibility

- Palettes: default, deuteranopia, protanopia, tritanopia, high contrast. Only semantic colours
  (ok / warn / danger / cash) change; damage colours use CSS `color-mix` against the palette, so
  every severity scale follows it. Status is never colour-only (labels, icons, hatching).
- UI scale 75–150 % (everything is sized in `em` off one root size).
- Reduce motion: no flashing light bars or blinking warnings.
- Large scanner text; km/h or mph.
- Keyboard: every control is a real button; Esc backs out; the binding dialog takes Shift+Esc to
  bind Escape itself.

## Performance (headless Chromium 141, 4-core cloud container, mock playing at 1×)

| Screen | Frames in 3 s | Frame p50 / p95 | Long tasks |
|---|---|---|---|
| HUD (minimap + silhouette, re-render every frame) | 181 | 16.7 / 16.7 ms | 0 |
| Captain map (canvas redraw every frame, heat map rebuilt at 4 Hz) | 181 | 16.7 / 16.7 ms | 0 |
| Main menu | 181 | 16.7 / 16.7 ms | 0 |

Bundle (demo, incl. inlined fixtures, roster, mod catalog and city maps): 1.42 MB JS / 256 kB gzip,
30 kB CSS. Fixtures and data dominate (the recorded `captain.jsonl` alone is 410 kB); a host that
only mounts the kit with a real provider would not bundle them.

## Layout

```
src/
  index.ts            public API            mount.tsx / App.tsx   mount + root
  model/              GameModel, UiState, UiCommand, bindings, proposed contract types
  generated/          contracts.ts (generated)
  data/               real data loaders (catalog), economy + estimator ports
  mock/               simcli run parsing, event reducer, mock provider, toy police, mock content
  screens/            one folder per area (hud, menu, settings, garage, race, impound, crew, police, captain, replay)
  ui/                 theme.css, primitives (kit.tsx), formatting, map geometry
demo/                 the demo page (npm run demo)
e2e/smoke.mjs         headless smoke test + perf probe
scripts/              gen-types, gen-fixtures, gen-synthetic
tools/gen-runtime-schemas   Rust: runtime contract schemas
tools/record-police         Rust: records a real wp_police run as a fixture
```

## Known gaps

- **Mock data where nothing is wired yet**: race progress, crews/territory, lobby roles, results.
  Police is real in `police_pursuit` (E's own city, since E has not moved to D's road network) and
  a toy elsewhere. See `docs/integration/ui.md`.
- Region damage for real runs is **inferred** from contact geometry (contact point vs. car pose
  from metrics) when a stream has no `RegionDamaged` (contract v0.2.0; `wp_damage` does not emit
  it yet). `PartDetached` group ids are
  mapped with a guess (debug lattices: 1 = front, 2 = rear).
- simcli `metrics.csv` has no heading column; heading is derived from motion (wrong for a moment
  when a car reverses after a crash).
- Repair quotes / mod previews are ports; small drift from `wp_economy` is possible until the
  real preview is exposed through wp_wasm.
- The binding flow is tested with the keyboard in CI; gamepad/wheel capture uses the Gamepad API
  and was not exercised headless (no device).
- Display font falls back to system condensed sans (no web fonts are bundled).
- The kit is not wired into `apps/testbed-web` (owned by 04): it provides the mount API and demo.
