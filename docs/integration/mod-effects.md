# Mod effects integration notes (dead and half-working mods)

- **Workstream:** owner bugs "my runflats don't work as intended" and "check for other no-op mods" (2026-10-26)
- **Branch:** `claude/project-thread-gzll20` (PR #96)
- **Crates touched:** `wp_vehicle` (run-flat constant + helper), `wp_sim` (per-vehicle run-flat and cooling), `wp_economy` (steering rack, fake-plates carry), `wp_wasm` (career wiring, map, audio scanner), `apps/ui-kit` (map tool icons), `apps/testbed-web` (career scanner, radio strip, 3D spikes source)
- **Contract version built against:** unchanged (no `wp_contracts` edit)

## What was wrong
`wp_economy::apply_mods` filled `ToolLoadout` (radar detector, scanner, run-flat, reinforced
cooling, fake plates, nitrous) but nothing read it, so those mods were bought for nothing. Two
more only half worked: Wider Tires (width only feeds the ground probe and aquaplaning: no dry
grip, only the drag penalty) and the Steering Angle Kit (its quicker ratio only reached force
feedback). Every `Vehicle` and `Durability` effect was already applied and read.

## What each mod does now
| Mod | Effect | Where |
|---|---|---|
| Run-flat Inserts | A punctured tyre (spikes, crash, carried flat) never deflates past 0.2 for the handling: grip × 0.9, sits ~6 % of its radius lower, drag × 1.8, never on the rim | `wp_vehicle::vehicle::RUN_FLAT_MAX_DEFLATION`, `Sim::set_run_flat` |
| Aluminium Radiator, Oil Cooler (tier 1) / Race Radiator (tier 2) | A holed radiator heats the engine 0.7× / 0.5× as fast (radiators still toughen the radiator mount) | `wp_sim::COOLING_OVERHEAT_SCALE`, `Sim::set_cooling_tier` |
| Radar Detector (1) / Long-range (2) | In a career run the map shows police only in plain sight (120 m), on the player, or in the air; the detector adds patrolling / chasing units within `radar_range_m[tier]` (300 / 500 m) | `wp_wasm::map::Seen`, `PoliceSystem::radar` |
| Police Scanner (1) / Decrypting Scanner (2) | The radio is heard at audio tier Pro / Elite (6 s / 3 s delay); no scanner = no radio at all, voice or strip. The Decrypting Scanner puts every roadblock and spike strip on the map with an icon | `wp_wasm::audio::career_scanner_tier`, `map::TOOL_INTEL`, `ui-kit drawToolIcon` |
| Fake Plates | Half the heat a free run would carry follows the car into the next run (what police saw during the run still counted) | `Economy::carry_heat_with_plates` |
| Wider Tires | +4 % grip (2.0 per metre of extra width over stock) | `wp_wasm::career` `WIDTH_GRIP_PER_M` via `set_tire_condition` |
| Steering Angle Kit | The quicker ratio also speeds the rack (`rack_speed_rad_per_s / ratio_scale`) | `wp_economy::mods::apply_vehicle_effects` |
| Nitrous Kit | Not here: the nitrous workstream builds the boost and hooks it to the mod | — |

Physics effects (run-flat, cooling, grip, rack) are the same in Arcade and Simulation.

## Exposes
- `wp_sim::Sim::set_run_flat`, `set_cooling_tier`, `tire_deflation`, `COOLING_OVERHEAT_SCALE`.
- `wp_wasm::session::PlayerGadgets`, `Session::set_gadgets` / `gadgets`.
- wasm: `player_gadgets()` (`[radar, scanner, fake_plates]`, empty outside the career),
  `map_dynamic_world(player)` (every unit and tool, for the 3D spike strips).
- `map_dynamic()` tools: `kind` + 2 (`TOOL_INTEL`) when the decrypting scanner reported it;
  `MapTool.intel` in `apps/ui-kit/src/map/data.ts`.
- `tire_punctures()` and the flat-tyre audio read the capped deflation.

## Behaviour kept on purpose
- Sandbox scenarios (no career) still show every unit and tool and hear the radio as the audio
  panel picks: the gadget filter applies only to career runs.
- Spike strips still puncture run-flats (blow-out pop, `SpikeHit`), and a spiked run-flat is
  still recorded flat after the run.

## Tests
- `crates/wp_sim/tests/run_flat.rs`: four flats, 6 s full throttle, both modes: plain 4.9 m/s on the rims every tick, run-flats 14.3 m/s, never on the rims.
- `crates/wp_sim/tests/cooling.rs`: 90 km/h into a wall holes the radiator; engine heat 20 s later 0.213 stock vs 0.107 race radiator.
- `crates/wp_wasm/src/map.rs` `career_map_shows_police_by_sight_and_radar_detector`.
- `crates/wp_economy`: `steering_angle_kit_quickens_the_rack`, fake-plates carry in `profile.rs`.
- `apps/ui-kit/src/map/data.test.ts`: the intel bit decodes.

## Known gaps
- Scenario `mods` in `apps/simcli` (non-career) build the modded car but do not set run-flat,
  cooling or gadgets; only the career does.
- The audio panel's scanner dropdown is ignored during a career run (the car's scanner wins).
- The garage stats estimator does not count the wider tyres' grip.
