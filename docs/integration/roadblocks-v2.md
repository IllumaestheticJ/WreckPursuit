# Roadblocks v2 integration notes (arcade-classic-style roadblocks and spike strips)

- **Workstream:** integration lead — roadblocks-v2 (owner request 2026-10-31: "redo police road
  blocks / spike strips" to match the 2005 arcade pursuit classic; never named in this repo)
- **Branch:** `lead/roadblocks-v2` (from `main` 1abfd2a4)
- **Crates / folders touched:** `wp_police` (tools, dispatch, race, chatter, captain, config),
  `data/police/{tools,presence}.toml`, `wp_sim` (police_tools, police_link, lib, physics),
  `wp_wasm` (police_tools export, map, police_status), `apps/simcli` (scenario order, metrics,
  scenarios), `apps/testbed-web` (scene, HUD alert, e2e).
- **Contract version built against:** 0.4.2 (no contract change).

This **replaces** the owner's 2026-10-28/29 roadblock rules (barrier sections wall to wall, one
wooden sawhorse barricade in the only gap, light water-filled barriers).

## What a roadblock is now
- A line of real police cars parked across the road at an angle (nose toward the racer,
  neighbours nose to nose, odd cars of three or more staggered 1.2 m), lights flashing, brakes
  and handbrake held (`PoliceSystem::unit_input` for a parked unit). They are ordinary police
  units spawned by `wp_sim` as vehicles: same vehicle and damage code as every car, no special
  physics, nothing branches on `DamageMode`.
- The ways through: the gaps between cars (≥ 2.5 m, a car width plus a little; the L1–2 gap is
  4–8 m), open lanes at the kerbs on a road too wide for the tier, the light end of a cruiser
  (ram its nose or tail corner and it spins aside), or off the road. Heavy units
  (`Unit::heavy`, `sentinel_bastion` 2.6 t SUV: `Sim::set_police_heavy_car`, else the fleet's
  heaviest model) barely move.
- Tiers (`data/police/tools.toml` `[[roadblock.tiers]]`, one per wanted level):

  | Level | Front cars | Heavy | Gap | Spiked gaps | Second line |
  |---|---|---|---|---|---|
  | 1 | 2 | 0 | 4.5 m | 0 | – |
  | 2 | 2 | 0 | 4.0 m | 0 | – |
  | 3 | 3 | 0 | 3.0 m | 1 | – |
  | 4 | 3 | 2 | 2.8 m | all | 35 % |
  | 5 | 4 | 3 | 2.7 m | all | 70 % |

  The layout (`tools::roadblock_layout`) fits the road: fewer cars where they cannot stand
  angled (footprint ≥ 4.4 m) with car-width gaps, wider gaps and open kerb lanes where it is too
  wide. A second line stands 14 m behind with one car fewer, its gaps off the front line's. A
  spiked gap has a strip 3.5 m in front of the car line, 1 m past each end of the gap. Cones in
  front of every car, flares 22 m out (drawing only).
- Roadblocks unlock at L1 now (`presence.toml` `roadblock_level` 3 → 1).
- Placement: on the racer's predicted path as before; a site is taken only when **every car
  spot is out of every racer's view** (`in_racer_view_opt`, with the world's sight lines on the
  AI captain path). The cars appear together, once all spots are hidden
  (`maintain_tool_cars`); only then is the block set up (gap strips down, on the map, HUD alert).
- After the racer is 15 m past the last line (within 40 m across), the block breaks up: gap
  strips lifted, its cars rejoin the pursuit (role `Reinforcement`, `Pursue`) or go out of
  service and drive off; `PoliceStats::roadblocks_passed`. Expiry is unchanged (lifetime 90 s).
- Radio: the tier-1 dispatch call names it ("three-car roadblock with spikes 400 metres
  ahead, …"). HUD: `police_status().tool_ahead` → "⚠ ROADBLOCK AHEAD · three-car roadblock with
  spikes · 320 m" (`#tool-alert`, within 400 m). Map / minimap: only set-up blocks, sized by
  their cars (careers: near or scanner intel, as before).

## Spike strips
- Laid by their unit (row 165 rules unchanged: no strip without its car beside it, out of
  view), across the racer's lanes from under the car's inner side to the kerb
  (`tools::spike_layout`): on a two-way road the car stands astride the centre line, the
  strip covers the racer's half; on a one-way road the car stands at the left kerb. Ways past:
  the oncoming lane, off the road, or ramming the car. Two flares on the approach.
- Roadblock gap strips are `SpikeStrip`s with `block: Some(id)` and no unit: they lie while
  their block stands, do not count against the spike budget and are announced with the block.
- Puncture / deflation unchanged (wheel crossing in `wp_police`, effects merged in `wp_sim`).

## Removed
- `wp_sim::police_tools` barrier bodies, the barricade and its pieces, `loosen_barriers`,
  `break_barricades`, `probe_box`, `ProbeFilter`, `ToolPart`, `BarrierView`,
  `roadblock_barriers()`, `barricades_broken()`, `BARRICADE_*`, `physics::ROADBLOCK_GROUP`.
  The state hash still writes the (always 0) barrier count so every other run keeps its hash.
- `wp_police::tools::{Barricade, GAP_W_M, BARRIER_OVERLAP_M, STOP_OVERLAP_M, BARRICADE_*}`,
  `Roadblock::{barriers, barricade}`, the sealable-site rule, spike `extra` segments (field kept,
  never filled), config `roadblock.{units, barrier_half_extents_m, barrier_mass_kg}`.
- wasm `police_tools()` kinds 0/2/3; testbed barrier and sawhorse meshes.

## Exposes
- `wp_police`: `tools::{BlockCar, BlockGap, BlockRecipe, BlockLayout, roadblock_layout,
  spike_layout(site, sec, two_way)}`, `Roadblock::{level, cars, gaps, gap_strips, cones, flares,
  set_up, heavy_cars(), radio_name(), ahead_of(), past_by()}`, `SpikeStrip::{block, flares}`,
  `Unit::heavy`, `PoliceSystem::{place_roadblock_at(racer, pos, dir, level), tool_ahead()}`,
  `RoadblockConfig::tier()`, `dispatch::{BLOCK_PASSED_M, BLOCK_PASS_SIDE_M,
  FLAT_ROAD_HALF_WIDTH_M}` (the flat test world's tool road is a 19 m boulevard).
- `wp_sim`: `Sim::{set_police_heavy_car, tool_markers}`, `ToolMarker`, `ToolMarkerView`.
- simcli: `roadblock_at(…, level: n)` (default 2); flat-world scenarios with a direct tool order
  get physical police cars; world metrics `roadblock_cars_max`, `roadblock_heavy_max`,
  `roadblock_car_moved_max_m`, `roadblocks_passed` replace `roadblock_barriers_max`,
  `barrier_displaced_max_m`, `barricades_broken`.
- wasm: `police_tools()` kinds 1 (strip), 4 (cone), 5 (flare); `police_status().tool_ahead`.

## Contract change requests
None needed.

## Changes outside my crates
`apps/simcli` (owned by 10 harness): the `level` field on `roadblock_at`, the new world metrics
and the flat-world police cars, documented in `apps/simcli/SCENARIOS.md`.

## Known gaps
- Narrow roads (< ~12 m) take fewer cars than the tier asks for (two at most below ~16 m);
  the L5 four-car line needs a ~25 m road.
- The broken-up block's cars pursue only on worlds with a road graph (the flat test world keeps
  them parked).
- Cones and flares have no bodies.
