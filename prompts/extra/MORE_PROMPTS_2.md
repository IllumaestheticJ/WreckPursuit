# WreckPursuit — more prompts for other agents (P12–P19)

Paste **one** prompt into a new Claude Code session on the WreckPursuit repo. Each prompt works on its own branch and stays out of areas other sessions are already working on. The lead merges each branch when it's done.

These prompts deliberately avoid areas that already have a session:
- keyboard countersteer / drift (A and Drift Lab B)
- police (E, radio, siren)
- traffic and parked cars (D)
- road widths and residential districts (city helper)
- Arcade damage (B)
- AI racers (opponent-pace)
- nose dive (brake-dive)
- frame rate (browser-perf)
- shift times (H)

Shared rules (already included in every prompt below): read CLAUDE.md and docs/INTEGRATION.md; Arcade and Simulation differ **only** in car damage; deterministic sim (no wall clock, seeded RNG, no HashMap iteration); no licensed car or brand names; low-poly dark art style; never create pull requests.

---

## P12 — Chase camera feel

```
You are working on WreckPursuit (repo IllumaestheticJ/WreckPursuit). git fetch origin; create branch lead/camera-feel from origin/lead/round12 (not main); push only there; no pull requests. Read CLAUDE.md, docs/INTEGRATION.md and apps/testbed-web/src/cameras.ts (+ the camera rig in main.ts).
Goal: a chase camera that feels like a great arcade racer while staying readable in pursuits.
- Spring-damped follow, separate for position and look-at. The car rotates slightly ahead of the camera through corners and the camera swings out in drifts (look into the slide direction, limited).
- Speed FOV (a few degrees with speed), subtle high-speed shake, impact shake and a short hit-stop on big crashes, scaled by impact energy (from sim events; cosmetic only, never touches the sim).
- Collision with buildings and tunnel ceilings: no clipping. Pull in smoothly, never snap.
- Look-back and look-left/right bindings work. After a spin the camera doesn't spin wildly: it recentres on the travel direction with a short delay.
- Settings: camera distance, height, FOV, shake strength (0–100%), in the existing settings menu (apps/ui-kit menu).
- e2e: scripted drift, crash and tunnel drive → no NaN, no clipping (camera never inside a collider), frame time unchanged. Document in docs/integration/camera-feel.md.
Gate: fmt, clippy -D warnings, cargo test --workspace, wasm build, tsc, just web-e2e (CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0).
```

## P13 — Player HUD (game look, not debug)

```
You are working on WreckPursuit (repo IllumaestheticJ/WreckPursuit). git fetch origin; create branch lead/player-hud from origin/lead/round12 (not main); push only there; no pull requests. Read CLAUDE.md, docs/INTEGRATION.md, docs/art/STYLE_DECISION.md, apps/ui-kit (menu, live readouts, map), apps/testbed-web/src/hud.ts, ui/layout.ts and index.html (body.wp-player hides the dev panels).
Goal: the player mode (developer panels off) gets a proper game HUD in the low-poly dark style.
- Analogue-style speedo/tach with a gear indicator and shift light (bottom right, next to the minimap).
- A compact car-health silhouette (from the existing damage view) that flashes the damaged area.
- Heat/wanted meter with an evade/bust progress ring during pursuits (data already in police_status). Big pursuit banners stay but get shorter and fade.
- Money/rep ticker with a short "+$" popup for bounty/escape.
- The radio strip (from lead/police-radio-v2 if merged; otherwise leave a slot).
- Damage/repair notices.
Layout must work at 16:9, 21:9, 4:3 and phone landscape (the touch layout exists — no overlaps). Keep the developer HUD unchanged when the dev panels are on. Respect prefers-reduced-motion. e2e: the layout checks at 4 sizes, no overlaps, no off-screen elements. Document in docs/integration/player-hud.md with screenshots.
Gate: tsc, just web-e2e, wasm build if touched.
```

## P14 — Style points: near misses, drifts, air, takedowns

```
You are working on WreckPursuit (repo IllumaestheticJ/WreckPursuit). git fetch origin; create branch lead/style-points from origin/lead/round12 (not main); push only there; no pull requests. Read CLAUDE.md (deterministic sim rules), docs/INTEGRATION.md, crates/wp_sim events, wp_economy (money/rep), wp_police (heat).
Goal: a skill/score layer that rewards spectacular driving during free roam and pursuits.
- Detected in the sim (deterministic, from state, no wall clock): near misses with traffic and police (< 0.7 m at speed, no contact), sustained drifts (angle × speed × time), air time, oncoming-lane driving, police takedowns (a unit wrecked by contact you caused), and clean escapes. Combo multiplier with a short window; a crash breaks the combo.
- Score feeds rep (and a little money) in wp_economy. Keep the numbers in a data file (data/economy/style.toml).
- Events go out through the existing event stream. The testbed shows popups ("NEAR MISS +150", "DRIFT x3") in the player HUD style.
- Doesn't change physics or AI. Same in both damage modes.
- Tests: scripted near miss, drift and jump scenarios produce the expected events within ranges; a determinism hash test. Document in docs/integration/style-points.md.
Gate: fmt, clippy -D warnings, cargo test --workspace, all scenarios, wasm build, tsc, just web-e2e (CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0).
```

## P15 — Breakable street props

```
You are working on WreckPursuit (repo IllumaestheticJ/WreckPursuit). git fetch origin; create branch lead/breakable-props from origin/lead/round12 (not main); push only there; no pull requests. Read CLAUDE.md, docs/INTEGRATION.md, crates/wp_sim (props: parked cars and cones as dynamic bodies, propsMoved), crates/wp_world (city furniture, if any), apps/testbed-web/src/render/props.ts and docs/integration for props/draw-props.
Goal: city furniture you can smash, Wreckfest-style.
- Lamp posts, traffic sign posts, bins, hydrants (with a water spray FX), fences/hedges, bus shelters, benches, plus the cones that already exist. Placed by deterministic rules along sidewalks and junctions from the city data.
- Each prop has a mass class and a break threshold. Light props get knocked away and slow you slightly. Heavy posts bend or snap above an impulse, and below it they stop you like a pole.
- Damage to the car scales through the normal contact path. No special-casing by mode (only the wp_damage tables differ).
- Rendered instanced, with LOD like the parked props. Within the browser performance budgets (e2e/perf_check.mjs).
- Sleeping bodies until touched; no per-frame cost when idle.
- Tests: a scenario hitting each prop class at 30/60 km/h (break / no-break expectations, ranges), a determinism hash, perf numbers in the doc. Document in docs/integration/breakable-props.md.
Gate: fmt, clippy -D warnings, cargo test --workspace, all scenarios, wasm build, tsc, just web-e2e (CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0).
```

## P16 — Night city lighting

```
You are working on WreckPursuit (repo IllumaestheticJ/WreckPursuit). git fetch origin; create branch lead/night-lighting from origin/lead/round12 (not main); push only there; no pull requests. Read CLAUDE.md, docs/art/STYLE_DECISION.md (low-poly dark), apps/testbed-web/src/render (style.ts, palette.ts, post.ts, quality.ts, city_tiles.ts, fleet.ts), scene.ts. The browser-perf session owns frame-time budgets: stay within them.
Goal: night streets that look moody and readable, without real-time lights per lamp.
- Street-lamp light pools as cheap projected decals or vertex-lit road tiles, plus lit building windows (an emissive pattern by district, some off), neon in commercial zones.
- Headlight cones on the road for the player and nearby cars (one shadowless spot for the player, decals for others).
- Police lightbar glow bouncing on nearby surfaces (the existing single siren light, improved).
- Wet-road reflections of lights in rain (screen-space or a cheap planar fake).
- Fog/horizon colour by time of day.
- All quality-tiered (low/medium/high, with a software-GL fallback). Measure with e2e/frame_profile.mjs before and after; no regression on the high tier beyond the budget. Screenshots in docs/art/night/. Document in docs/integration/night-lighting.md.
Gate: tsc, just web-e2e, perf_check.
```

## P17 — Car customisation (visual)

```
You are working on WreckPursuit (repo IllumaestheticJ/WreckPursuit). git fetch origin; create branch lead/car-visuals from origin/lead/round12 (not main); push only there; no pull requests. Read CLAUDE.md (no licensed names or brands), docs/INTEGRATION.md, wp_economy (garage, mods, money), apps/ui-kit garage (CareerGarage.tsx), apps/testbed-web/src/render (panels.ts, fleet.ts, palette.ts).
Goal: the player can make their car look like theirs.
- Paint: colour picker with a curated palette plus custom, finish (gloss/matte/metallic in the low-poly dark style), two-tone.
- Wheels: a few low-poly rim designs and colours.
- Simple livery: stripes, numbers, a few generic decals (no brands).
- Cosmetic body kits per car where panel meshes allow (front lip, wing). If a part changes aero or mass, it goes through the existing mod system, not a hidden change.
- Prices in data (data/mods/cosmetic.json), bought in the garage, saved with the career. Visible on the car in the world, in replays, and to other players in net play (serialise the look in the snapshot / player info).
- Tests: economy purchase/apply/save tests; e2e: buy paint → the car colour changes in the world; net two-tab test sees it. Document in docs/integration/car-visuals.md with screenshots.
Gate: fmt, clippy -D warnings, cargo test --workspace, wasm build, tsc, just web-e2e.
```

## P18 — Career loop: jobs and contracts

```
You are working on WreckPursuit (repo IllumaestheticJ/WreckPursuit). git fetch origin; create branch lead/career-jobs from origin/lead/round12 (not main); push only there; no pull requests. Read CLAUDE.md, docs/INTEGRATION.md, the GDD under docs/ (career / economy sections), wp_economy (money, rep, impound, repair billing, dealer), wp_race (races), wp_police (heat, bounty), apps/ui-kit garage and menu.
Goal: a reason to drive. Give the career a loop of short jobs in the city.
- Job types: deliveries against the clock (fragile cargo: damage reduces pay), getaway jobs (start with heat, reach the drop), street races (existing), heat challenges (survive L3 for 90 s), and repo jobs (steal a parked car, bring it to the yard).
- A job board in the garage plus map markers (map POIs). Payouts and rep in a data file. Difficulty unlocks with rep. Failure costs (impound fees, repair bills) use the existing economy.
- Jobs start and end in the sim deterministically: objectives as sim-side triggers, results as events.
- Save/load with the career.
- Tests: each job type completes and fails in a scripted scenario, payouts are correct, saves round-trip. Document in docs/integration/career-jobs.md.
Gate: fmt, clippy -D warnings, cargo test --workspace, all scenarios, wasm build, tsc, just web-e2e (CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0).
```

## P19 — Traffic variety: buses, trucks, emergency vehicles

```
You are working on WreckPursuit (repo IllumaestheticJ/WreckPursuit). git fetch origin; create branch lead/traffic-variety from origin/lead/round12 (not main); push only there; no pull requests. Read CLAUDE.md, docs/INTEGRATION.md, data/cars/README.md (analogue names only), data/cars (roster, build_roster.py), crates/wp_world traffic, apps/testbed-web/src/render/fleet.ts. Machine D is retuning traffic density and parking in ws/world right now: don't touch density/parking; add vehicle types and behaviours only, and merge origin/ws/world before you push.
Goal: a city that feels alive.
- New civilian types: city bus (stops at bus stops in bus lanes), box truck / delivery van (double-parks briefly with hazards), taxi (pulls over at random kerbs), garbage truck (slow, stops often) and motorbike (lane-splits, light).
- Emergency vehicles: ambulance and fire engine on sirens occasionally crossing the city; traffic yields to them like it does to police.
- Each has car data (mass, lattice, panels mesh; low-poly dark) and goes through the same physics and damage code paths. Heavy vehicles are dangerous to hit.
- Spawn mix by district and time of day, in data.
- Tests: spawn mix within ranges, bus stops served, yielding to emergency vehicles, perf within budgets with the new meshes (instanced). Document in docs/integration/traffic-variety.md.
Gate: fmt, clippy -D warnings, cargo test --workspace, all scenarios, wasm build, tsc, just web-e2e (CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0).
```
