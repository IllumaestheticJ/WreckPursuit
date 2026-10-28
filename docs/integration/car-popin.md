# Lead helper: Free Roam car pop-in (branch `lead/car-popin`, 2026-10-28)

- **Workstream / prompt:** lead helper (traffic spawning, LOD, render distances; physics frozen)
- **Branch:** `lead/car-popin` (from `main` 7fa64d8a; not merged, no PR)
- **Crates / folders touched:** `wp_world` (traffic), `wp_wasm` (street racers, free-roam roamers),
  `apps/testbed-web` (render quality, e2e), `docs/media/popin/`
- **Contract version built against:** 0.3.0 (no contract change)

Owner report: "cars also render in free roam too close".

## What was wrong
The reading "cars pop into existence close to the player instead of being visible far down the
road" was right; the chase camera is not involved. Three causes:

1. **Traffic spawned and despawned in plain view.** The only rule was "nothing within 320 m of a
   player" (`TrafficConfig::sight_radius_m`). Past 320 m anything went, and the testbed's daytime
   fog (FogExp2 0.0016) hides only ~23 % of a car at 320 m (~11 px long at 720p). On the combined
   world (Port Vesper + Ashpeak) about one new car in ten appeared where the chase camera had a
   clear line of sight to it, 40–85 a minute on a highway drive, 320–500 m away (sim
   measurement). In the browser (`docs/media/popin/`, 24 s of chase camera): 6 cars appeared or
   vanished in plain view 355–432 m from the player, e.g. headlights switching on 355 m down the
   downtown avenue at night (`before_vesper_downtown_night_zoom.png`).
2. **Street racers and free-roam roamers popped too.** Wrecked/stuck street racers and shift
   changes (`wp_wasm::street`) and stranded roamers (`wp_wasm::roam` rescues) vanished and came
   back anywhere ≥ 150 m from the player, distance only, no sightline check.
3. **The low and touch graphics profiles cut traffic short of the city.** Instanced traffic past
   `trafficM` was not drawn: low 600 m inside a 900 m city, touch 300 m inside 600 m. With cars
   now kept alive while in view out to 1000 m, they would have blinked out there.

Not causes: parked cars are all placed when the world is built (whole map) and the browser draws
every one (`render/props.ts`); nothing loads in by area. Sim LOD promotion (80 m) swaps to the
full car on the same frame (row 25 fix) and is not visible pop-in. The high profile had no
traffic cutoff.

## What changed
- **`wp_world` traffic: line-of-sight veto.** New `TrafficConfig::view_radius_m` (1000 m). Past
  `sight_radius_m` an agent can still be neither spawned nor despawned where a player's driver
  eye (1.3 m) or chase camera (near 6 m back / 2.2 m up, far 10 m / 3 m) has a clear
  `CityMap::line_of_sight` (terrain, buildings, decks, tunnels, structures) to its roof, now or
  1 s ahead. Cars appear behind buildings, crests and bends, or beyond 1 km (≥ 92 % fogged, ~3 px
  at 720p), never on screen. Applies to spawn spots, density/area/stuck despawns and wreck
  removal. Bounded cost: at most `LOS_BUDGET` (160) tests per 0.25 s spawn update (+24 for wrecks);
  past it every remaining car counts as seen until the next update. Density control is unchanged
  (the veto only moves where cars appear).
- **`World::in_player_view(pos)`**: the same rule (320 m all round or line of sight within
  1000 m), public for anything else that makes a car appear or vanish.
- **`wp_wasm::street`**: respawns of wrecked/stuck racers and shift changes only take a racer
  away when the player cannot see it and put it on a pose the player cannot see
  (`out_of_view`, up to 8 tries, else the move waits a tick).
- **`wp_wasm::roam`**: a stranded roamer is rescued only out of the player's view, onto the first
  of 8 candidate slots out of view.
- **Testbed render distances**: instanced traffic is drawn as far as the city on every profile
  (`Quality.trafficDrawM` = the profile's draw distance; the separate `trafficM` is gone).

| Profile | City draw distance | Traffic drawn to (before → after) | Traffic spawn/despawn (before → after) |
|---|---|---|---|
| high (PC) | camera far 3000 m, fog-limited (~1690 m day, ~640 m night) | no cutoff → no cutoff | ≥ 320 m → ≥ 320 m and out of sight within 1000 m |
| low (software GL) | 900 m | 600 m → 900 m | same |
| touch | 600 m | 300 m → 600 m | same |

Mid / proxy meshes are unchanged (proxies past `fullLodM`: high 400 m, low 120 m, touch 100 m),
so the extra far cars are low-poly instances.

## Tests
- `crates/wp_world/tests/popin.rs` (≈ 18 s debug):
  - `free_roam_traffic_never_appears_where_the_player_can_see_it`: six 40 s drives on the combined
    world (highway / city, noon / night, seeds 1 and 7). **N = 320 m**: no spawn closer, in any
    direction (mirrors, rear and orbit cameras). And no spawn in the line of sight of the near or
    far chase camera within 1000 m (the frustum is inside that test). Cameras are placed by the
    test itself (5.6 m / 1.8 m and 10 m / 2.9 m, body and roof points), after the step.
    Measured: 0 of 2221 spawns in view; 2 of 2217 despawns in view (dead ends, see gaps), ≤ 1 %
    allowed.
  - `line_of_sight_veto_keeps_the_streets_busy`: cars within 150 m 108 vs 124 (87 %) and cars
    in view 200 vs 207 (97 %) of the radius-only rule; ≥ 70 % required.
  - `parked_cars_never_appear_during_play`.
  - `classify_browser_popins` (ignored): line-of-sight classification of the browser runs.
- `wp_wasm::street` `wrecked_or_stuck_racer_is_moved_out_of_sight` now also asserts the new pose
  is out of the player's view.
- Render cutoff: `e2e/popin_shots.mjs` records every traffic car within the cull distance that
  the LOD director did not draw (`undrawnWithinCull`; `CHECK=1` fails on any). 0 in all 8 shots.

## Screenshots (`docs/media/popin/`)
Free Roam (`?roam=10`), chase camera, 1280×720, gfx high, 6 s per shot; see its README.

| Shot | Before: appeared / vanished in plain view | After |
|---|---|---|
| Port Vesper downtown avenue, 13:00 (~300 cars in reach) | 1 / 0 (363 m) | 0 / 0 |
| same, 22:00 | 3 / 1 (355–432 m) | 0 / 0 |
| Ashpeak Route 7, 13:00 (~20 cars in reach) | 1 / 0 (376 m) | 0 / 0 |
| same, 22:00 | 0 / 0 | 0 / 0 |

Cars still appear inside the camera's view cone (31–38 per downtown shot) but all behind cover
(grey rings).

## Perf numbers
PERF_PLACEHOLDER

## Goldens
GOLDENS_PLACEHOLDER

## Known gaps
- A traffic car that reaches the end of a road with no way on (a dead end at the map edge) is
  removed wherever it is, even in view (0.1 % of despawns in the test drives).
- The veto knows terrain, buildings, decks, tunnels and structures, not trees, props or other
  cars (conservative: more counts as seen). Session start (`populate`) keeps its 150 m rule.
- Spots with long clear sightlines and little traffic (Loop 9 has 5–8 cars in reach by day, in
  both builds) can only fill from hidden ramps and side roads; rural traffic is sparse anyway.
- No dedicated test drives a roamer rescue (no hook to strand one); it goes through the same
  `World::in_player_view` as the tested paths.
