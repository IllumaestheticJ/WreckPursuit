# Status

_Last updated: lead integration run 5, 2026-10-19. Contract **v0.2.0** (applied in run 3, see
`docs/CONTRACT_CHANGELOG.md`; v0.3.0 = typed police/economy/net events)._

Machines are named by letter: **A–I = prompts 02–10**, **J–Q = prompts 13–20** (started 2026-10-19 as
cloud sessions: J audio `ws/audio`, K weather `ws/weather`, L race/crews `ws/race`, M UI kit `ws/ui`,
N opponent AI `ws/opponent`, O meta services `ws/meta`, P design docs `ws/design-docs`, Q tuning
dashboard `ws/tuning-dash`).

## Workstreams

| Machine | # | Workstream | Branch | State | What works end-to-end on `main` | Next |
|---|---|---|---|---|---|---|
| A | 02 | Vehicle | `ws/vehicle` | merged | Real tire/suspension/drivetrain model in browser + simcli | tire compounds (H, G); review lead's `set_state`/`set_pose_and_speed`; rig stepper for F |
| B | 03 | Damage | `ws/damage` | **merged run 2** | Lattice deformation, parts detach as debris, collider follows crush, component health | T-bone crush low (0.09 m vs 0.15); frontal total-loss question |
| C | 04 | Testbed | `ws/testbed` | merged | Debug layers, cameras, replay, scenario menu; shows real deformation | scenario v2 natively, barriers, mode switch, traffic agents layer |
| D | 05 | World | `ws/world` | **merged run 2** | Procedural city + traffic + signals run inside `wp_sim` (`set_world`), props are physical | integer ticks instead of f64 time; procgen speed |
| E | 06 | Police | `ws/police` | **merged run 2** | Heat/bounty, dispatch, pursuit AI, tools, captain; own scenarios pass | switch to D's road network + AI controller; integer ticks; `wp_sim` unit wiring (lead) |
| F | 07 | Net | `ws/net` | **merged run 4** | Binary protocol, authoritative server, prediction + bots on the real vehicle, browser net client (`net.html`) | 4-player end-to-end test |
| G | 08 | Economy | `ws/economy` | merged | Ledger, garage, mods, repair quotes, payouts, impound | call sites when E/F land |
| H | 09 | Cars | `ws/cars` | merged | 13 cars drive deterministically | new tires, `gen-lattice` regeneration, VALIDATION sim columns |
| I | 10 | Harness | `ws/harness` | merged | 17 scenarios, goldens, lint, native-vs-wasm, benches, CI | `barriers`/speed in scenarios, real cities, traffic soak, F codec |
| J | 13 | Audio | `ws/audio` | merged (run 3–4) | `wp_audio` director crate + procedural packs (not wired to the browser yet) | integration doc; hook into testbed |
| K | 14 | Weather | `ws/weather` | merged (run 3–4) | `wp_weather` crate: schedule, wetness, grip/aquaplaning | integration doc; feed grip to vehicle via contracts |
| L | 15 | Race/crews | `ws/race` | merged (run 3–4) | `wp_race` rules, scheduler, stakes, crews; `data/events/` | integration doc |
| M | 16 | UI kit | `ws/ui` | merged (run 3–4) | `apps/ui-kit` against mock data | integration doc |
| N | 17 | Opponent AI | `ws/opponent` | merged run 4 | AI racers on D's controller; circuit / traffic scenarios | — |
| O | 18 | Meta services | `ws/meta` | merged (run 3–4) | `apps/meta` accounts/matchmaking/leaderboards API | one flaky test under load (`twenty_bots_…` heartbeat) |
| P | 19 | Design docs | `ws/design-docs` | merged run 3 | GDD, city zones, feature backlog (`docs/design/`) | — |
| Q | 20 | Tuning dashboard | `ws/tuning-dash` | merged (run 3–4) | sweep runner + presets (`tools/tuning-dash`) | integration doc |
| R | 21 | Car models & panels | `ws/panels` | started 2026-10-19 | — | per-panel meshes, hinged doors/hood/trunk swing, dangling bumpers |
| Lead | 12 | Integration | `main` | — | Contacts → damage, debris, hull, static boxes, city loading; contract v0.2.0 | police units as sim vehicles, F's `wp_sim` hooks, physical traffic, v0.3.0 |

## What you can play right now
Published testbed (private link, owner): https://claude.ai/artifact/EJWFJePAQyuPts6NY8XACL — or
`just web` → http://localhost:5173: drive any of the 15 cars with real handling; crash into the
other car in the head-on scenario and watch the lattice crumple (L / H layers).

## Checks on `main` (2026-10-19, cloud container)
fmt, clippy `-D warnings`, `cargo test --workspace`, wasm build (no native deps reach it),
determinism lint (0 errors), native == wasm on every scenario, `just web-e2e` all pass.

## Perf (native, release; full log `docs/PERF.md`)
| What | Measured | Budget |
|---|---|---|
| vehicle + Rapier + contacts, 24 cars | 0.029 ms/car/frame (run 1) | 0.40 ms/car |
| lattice (B's bench) | see `crates/wp_damage/README.md` | 0.25 ms/car |
| traffic 400 agents (D) | see `crates/wp_world/README.md` | 2 ms |

## Milestone checklist
- [ ] **Checkpoint 1 (Debug)**
  - [x] FWD + RWD debug cars drivable in the browser
  - [x] deforming lattice (B merged; visible in the testbed)
  - [ ] barrier tests pass in Arcade and Simulation — B's own barrier suite passes; simcli goldens
        need `barriers` in scenarios (I) and tuning (B)
  - [x] replay round-trips
  - [~] determinism suite green — locally yes; GitHub CI runs once `main` is pushed
- [ ] Checkpoint 2 (Greybox): city + traffic ✓ (near-player traffic physical); police ✓ as sim vehicles (not drawn yet);
      heat/escape/capture ✗; repair billing (G ✓, wiring ✗); 4-player server test (F merged; end-to-end test pending).
- [ ] Checkpoint 3 (Low-poly playable).

## Owner actions
None pending (branch protection on, stale branch deleted, all machines resumed for v0.2.0).

## Run 5 (2026-10-19)
Police units are real `wp_sim` vehicles in city runs (patrol → violations → pursuit → search
works in the sim), near-player traffic is physical. Not yet drawn in the browser (C, this
round). Lead rounds are now automatic every 2 h (`docs/LEAD_AUTOMATION.md`).

## Next (toward the single-player greybox demo)
C draws police/traffic + city-chase preset; M wires the heat HUD and repair bill; E/D share
roads and controller; G + lead wire capture/escape → economy.
