# Tunnels and covered roads (backlog B04) integration notes

- **Workstream:** lead helper, backlog B04 (owner request: "a real way to shake pursuit")
- **Branch:** `lead/tunnels-los` (from `lead/round12`)
- **Crates / folders touched:** `wp_world` (format, `covered`, `vesper`), `wp_sim` (colliders),
  `wp_police` (helicopter sightline height), `wp_net` (captain map), `apps/simcli` (scenario
  field + expectation + probe + scenarios), `tools/city-import` (greybox),
  `apps/testbed-web/e2e` (render check), `data/city` (regenerated)
- **Contract version built against:** 0.3.0 (no contract change; one request below)

## What

Racers can break line of sight in a tunnel: nothing outside a covered stretch of road sees in,
nothing inside sees out, and two cars in the same tunnel still see each other along the tube.

**Data — `wp_world` road format v3.** `RoadNetwork::covered: Vec<CoveredSpan>` (the road file
is `wp_world`'s own format, not `wp_contracts`, so this needed no contract change). A span is
`{ name, kind: Tunnel | CoveredTrench, road, centre (portal-to-portal road-surface
centreline), half_width_m, clearance_m }`. `ROAD_FORMAT_VERSION` 2 → 3 (`#[serde(default)]`
on the field, so old JSON dumps still parse once their version number is bumped);
`procedural_12x12.road.{bin,json}` regenerated. The design request C5 (`covered: bool` per
lane) is answered with spans instead: a lane runs junction to junction, a tunnel is part of one.

**Rules — `wp_world::covered`.** `CoverShape::contains(p)`: within the walls, between the
portals, from 3 m under the road up to the ceiling. `CoverShape::blocks(a, b)`:
- inside ↔ outside: blocked (dark portals, solid roof and walls);
- inside ↔ inside the same span: clear if the straight line stays inside (sampled every 2 m;
  a bend's wall blocks);
- outside ↔ outside: blocked where the line passes through the shell (walls + deck), clear
  above the deck.

`CityMap::line_of_sight` applies them before buildings and terrain, so **every** sightline
gets them for free: ground units (`sees` / `sees_at`), the helicopter, traffic cameras, and
the fog/rain cap in `wp_weather`'s adapter (which wraps the world query).
`CityMap::covered_los` / `World::set_covered_los(bool)` switches only the sightline effect off
(geometry stays) — the control half of the A/B scenarios. `World::covered_at(p)` /
`CityMap::covered_at(p)` → span index.

**Port Vesper v2 — two covered roads** (`wp_world::vesper::plan_covers`):

| Span | Road | Length | Profile | Inner half width / ceiling |
|---|---|---|---|---|
| **Hollins Tunnel** (`Tunnel`) | Loop 9, through the northern shoulder of Hollins Heights, (−720, 1377) → (−1110, 1399), climbing 41 → 57.5 m | 391 m | unchanged (Loop 9 already ran in a cut there); ground backfilled over the roof on the hill side | 12.6 m / 6.5 m |
| **Founders Cut** (`CoveredTrench`) | Uptown Connector, (418, 1005) → (413, 1255) | 250 m | dropped 7 m below the lowest natural ground (road at 16.2–16.5 m; internal profile anchor, ≈ 5 % ramps inside the 8 % arterial cap) under a plaza deck at ≈ 23 m; ground backfilled level with the deck | 10.5 m / 6 m |

Terrain: the normal carve leaves the road and its 7.5 m shoulder at road level (so the ground
never enters the tube); beyond `inner wall + 0.6 m wall + 8.5 m - 1 m` the ground is raised to
`min(natural, deck top)` out to 45 m. The 0.8 m roof deck reaches 8.5 m past each wall, over
that shoulder. Buildings whose lots fall on a deck or in a trench are dropped (3346 → 3312).
Route POIs (race grid kind) per tunnel: `"<name> entry"` (before the first portal, forward
direction), `"<name> lay-by"` (forward kerb lane mid-tunnel), `"<name> exit"`.

**Physics — `wp_sim::physics::city_ground_colliders`** (shared with `wp_vehicle::rig` client
prediction, so a predicted car hits the same walls): per centreline segment, a 0.6 m wall box
each side (from 1 m under the road to the ceiling) and the roof deck box.
`CityColliders::tunnel_boxes` = 249 on Port Vesper v2.

**Police — `wp_police`.** No tunnel-specific code: sightlines come from the world. One fix was
needed for the helicopter to matter at all: it looked at a fixed z = 1.3 m target, which is
underground anywhere above sea level on Port Vesper v2's terrain (terrain blocked every
sighting). It now uses the car's height (`eye_height_at(z)`, the same rule ground units use
since 07's fix). Flat cities are unaffected (`eye_height_at(z) = 1.3` for z ≤ 1).

**Captain map — `wp_net`.** `NetSim::covered_at` (default `false`; `WpSim` asks the world).
`ServerInstance::captain_view` keeps a covered racer's previous map entry (frozen where it
went in) instead of its live position; a racer the captain never saw outside is not shown.
There is no single-player helicopter/captain map; the helicopter itself loses the racer via
the sightline rules.

**Greybox — `tools/city-import`.** New materials `tunnel_wall` (both wall faces),
`tunnel_roof` (ceiling, deck top, deck edges, portal faces) and `tunnel_light` (1.6 × 0.5 m
panels every 12 m under the ceiling, **emissive** — `write_glb` now writes `emissiveFactor`
for any `*_light` material). Colours are taken from the low-poly dark palette in
`docs/art/STYLE_DECISION.md` / `ws/art`'s `palette.ts` (wall ≈ `retainingWall` 0x3c3f4b,
roof ≈ `bridge` 0x474a56, light ≈ `lampSodium` 0xffc27a). `data/city/vesper_v2.greybox.{glb,
b64.txt}` regenerated. **For machine S (ws/art):** please add `tunnel_wall`, `tunnel_roof`
and `tunnel_light` (glow like the lamp heads) to `CITY_MATERIAL_COLOURS` when you restyle.

## How to test

| What | Command |
|---|---|
| Sightline rules (straight, bend, portal, helicopter, through the shell) | `cargo test -p wp_world covered` |
| Vesper spans, LOS in/out, ground never in the tube, no buildings on decks | `cargo test -p wp_world --lib covered_roads_block_sight_in_and_out` |
| Colliders exist; a car driven at the Founders Cut wall stops at it, under the deck | `cargo test -p wp_sim --test city_v2` |
| Captain map freezes a racer at the portal | `cargo test -p wp_net --test captain_tunnel` |
| Drive through each tunnel at the limit (no contact, no air, on the road) | `simcli run apps/simcli/scenarios/city/tunnel_hollins_drive_through.ron` / `tunnel_founders_cut_drive_through.ron` (`slow`) |
| **Escape vs capture** | `simcli run apps/simcli/scenarios/police/tunnel_escape_founders_cut.ron` and `…_control.ron` (`slow`) |
| Watch a chase second by second (cover, status, LOS-lost timer, units, helicopter) | `cargo run --release -p simcli --example probe_tunnel_chase -- <scenario.ron> [every_ticks]` |
| Greybox has the tunnels; two framed shots | `just web-e2e` (`SHOTS=dir` writes `tunnels/founders_cut_north_portal.png`, `tunnels/hollins_tunnel_inside.png`) |

New simcli pieces (documented in `apps/simcli/SCENARIOS.md`): top-level `covered_los: bool`
(default `true`) and the expectation `first_pursuit_end(entity, outcome, min_s, max_s)`.

## Numbers

**Escape scenario** (`police/tunnel_escape_founders_cut.ron`, midday, racer L3, route
`Lantern Run -> Founders Cut lay-by` at 25 m/s): pursuit starts at 5.2 s, the helicopter
launches at 50.2 s and is over the racer (≈ 2 m) from ≈ 56 s while ground units fall 300+ m
behind. The racer enters the Cut at ≈ 97 s and parks mid-tunnel:

| Run | First pursuit end |
|---|---|
| tunnels block LOS | **Escaped at 126.9 s** (the L4 day escape time, 30 s, after entering), nearest unit ≈ 180 m |
| control, `covered_los: false` | **Captured at 142.0 s** (the helicopter never loses him; ground units arrive) |

Sweep of the racer's target speed (22 / 24 / 25 / 27 m/s): escape at 134.2 / 128.9 / 126.9 /
123.5 s with cover; capture at 167.7 / 149.5 / 142.0 / 141.2 s without — the contrast holds
across the sweep. (With cover, a unit that later drives into the Cut sees him inside the same
tunnel, starts a new pursuit and captures him at 144.7 s: the "units inside the same tunnel
still see each other" rule at work.)

**Drive-throughs** (`debug_sedan`, AI assists, at the limit): Hollins Tunnel 24.7 m/s max,
752 m, Founders Cut 18.2 m/s max, 447 m; both 0 off-road s, 0 airborne s, 0 crush.

**Release run times:** escape 17 s, control 22 s, drive-throughs ≈ 6 s each (all `slow: true`,
so debug `cargo test` skips them; the release CI jobs run them).

Screenshots (e2e, SwiftShader, greybox colours): `docs/media/tunnels/founders_cut_north_portal.png`,
`docs/media/tunnels/hollins_tunnel_inside.png`.

## Gate (2026-10-20, cloud container)

fmt ✓ · `clippy --workspace --all-targets -D warnings` ✓ · wasm build (`wp_wasm`, and
`npm run wasm`) ✓ · `tsc --noEmit` ✓ · `just web-e2e` (all checks, incl. the new tunnel check:
680 wall / 696 roof / 216 light vertices, lights emissive) ✓ · `cargo test --workspace`: all
pass except `simcli --test scenarios` (`goldens_match`, `all_scenarios_pass…`) — **both fail
identically on the base `origin/lead/round12`**: 11 stale goldens (the same scenarios, same
first-mismatch ticks and same new hashes on base and branch) and
`city/blocked_parked_player_vesper.ron`'s `traffic_turnarounds` (actual 0). So this branch
changes **no golden hash** and has no `[bless]` commit; re-blessing the round-12 goldens is a
lead-round job.

## Contract change requests

| # | Change | Why | Who is affected | Status |
|---|---|---|---|---|
| T1 | `WorldQuery::covered_at(&self, p: [f32; 3]) -> bool` (default `false`) | Sightlines already work through `line_of_sight`; this is for *flavour and doctrine*: the helicopter's "he's entering the tunnel, I lose him" radio line (L057), scanner static inside (J, design doc), police sending units to both portals (POLICE_DOCTRINE §4 Calloway) and the HUD telling the racer they are hidden. Meanwhile `wp_world::World::covered_at` and `wp_net::NetSim::covered_at` cover the callers that hold the concrete world. | D (implements, done locally), E (radio, doctrine), J (scanner), C / M (HUD), F (already has a local hook) | requested |

## Changes outside my crates

All of these are this branch's own work (lead helper), listed for the owners:
- `wp_world` (D): format v3, `covered.rs`, `CityMap::{covers, covered_los, covered_at}`,
  `World::{covered_at, set_covered_los}`, Vesper generator (plans, dip anchor, backfill,
  building filter, POIs), README.
- `wp_sim` (lead): tunnel colliders + `CityColliders::tunnel_boxes`; test.
- `wp_police` (E): `eye_height_at` for the helicopter target (pursuit.rs) and `sees_at`
  (lib.rs, same formula as before).
- `wp_net` (F): `NetSim::covered_at`, captain map freeze; test `tests/captain_tunnel.rs`.
- `apps/simcli` (I): `covered_los`, `first_pursuit_end`, `examples/probe_tunnel_chase.rs`,
  scenarios; `wp_wasm/src/scenario.rs` gets the new `Scenario` field.
- `tools/city-import` (D): greybox tunnels, `emissiveFactor` for `*_light` materials.
- `apps/testbed-web/e2e/run.mjs` (C): tunnel greybox check + two shots.

## Perf numbers

| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `CityMap::line_of_sight`, 100k eye-height pairs ≤ 250 m with one end on/near a tunnel road (worst case: every query near a span) | cloud container | 0.39 µs cover off → 1.04 µs cover on (7,318 pairs blocked by cover) | not measured | — |
| Away from tunnels (span bounding-box reject only) | cloud container | ≈ unchanged | — | police ≤ 0.5 ms/frame |

`cargo run --release -p wp_world --example tunnel_los_bench`. Police does a few dozen
sightline queries per tick, so even the worst case adds well under 0.1 ms per frame.

## Known gaps

- **Doctrine:** police do not yet treat portals specially (no "units to both portals", no
  holding the exits); a racer parked in a tunnel is eventually found by a unit that drives in.
  Needs T1 (or E calling `World::covered_at` via its own world handle).
- **Traffic cameras** sample at a fixed z = 1.3 m (`camera_sees` uses `sees`), so on Port
  Vesper's hills they are mostly blind already (terrain); not changed here (E).
- **Radio / audio:** no "entering the tunnel" line, no scanner static, no tunnel reverb (J).
- **Only the deck is a roof:** the portals are open quads, the gap between the walls and the
  backfill (under the deck overhang) is visible from very low angles at the portal ends.
  Fine for greybox; S may want headwalls.
- **Lights are emissive panels only** (no light pools on the road); with `ws/art`'s bloom
  they should read as lamps. No per-light real-time lights (style rule: emissive + bloom).
- **Calloway Tunnel / Lantern Underpass / parking structures** from the design docs are not
  in Port Vesper v2 (its loop does not cross a river mouth). Hollins Tunnel and Founders Cut
  stand in; more spans are a `plan_covers` table entry each.
- Underpasses under bridge decks (Route 40 North under Loop 9) are not covered spans; the
  deck already blocks the helicopter only where buildings/terrain do (it is not an occluder).
