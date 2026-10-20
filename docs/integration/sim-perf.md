# Sim step perf after the round-12b ride rework (`lead/sim-perf`)

- **Who:** lead helper, 2026-10-20.
- **Branch:** `lead/sim-perf` from `origin/lead/round12` (3c238cc).
- **Why:** after machine A's round 12b/13 vehicle work merged, the browser sim step in the city
  chase doubled: 120–145 ms per rendered frame against 65–76 ms before (`browser-perf.md` →
  "Merge with the touch-controls perf pass"). Browser-perf blamed Rapier cylinder shape casts in
  the wheel probes.
- **Touched:** `crates/wp_vehicle/src/ground.rs` (probe path only), `crates/wp_sim/src/lib.rs` +
  `physics.rs` (resting-prop wake test), two new examples (`wp_sim` `probe_cost`, `simcli`
  `vehicle_cost`), city goldens (`[bless]`). **Not touched:** steering, assists, drift, TCS,
  suspension, tyre model, car data, contracts, testbed TypeScript.

## Result

| Native, release, this container | Before (608a50a, pre ride merge) | `lead/round12` (3c238cc) | **This branch** | Budget |
|---|---|---|---|---|
| City chase, `wp_wasm --example chase_bench` (7 vehicles), ms/frame | 1.54–1.60 | 3.04–3.22 | **1.45–1.73** | — |
| One wheel probe in Port Vesper (`wp_sim --example probe_cost`, 20k lane points), µs | 0.57 (ray only) | 7.8–8.4 | **1.31** | — |
| Vehicle model + probes, Port Vesper, ms/car/frame: 1 · 8 · 24 cars (`simcli --example vehicle_cost`) | 0.092 · 0.071 · 0.046 | 0.245 · 0.231 · 0.297 | **0.062–0.077 · 0.063–0.070 · 0.057–0.067** | 0.15 |
| Whole frame, 24 cars in Port Vesper (no traffic / police), ms | 3.39 | 9.45 | **3.73–3.86** | — |
| Vehicle model + probes, flat ground, ms/car/frame: 1 · 8 · 24 cars | 0.033 · 0.029 · 0.027 | 0.060 · 0.062 · 0.063 | 0.055–0.065 · 0.052–0.066 · 0.055–0.063 (unchanged, see below) | 0.15 |

| Wasm | Before (608a50a) | `lead/round12` | **This branch** |
|---|---|---|---|
| Node 22 (V8, no rendering), same chase as `chase_bench`: weave · straight, ms per 60 Hz frame | 2.20–2.32 · 3.63 | 4.31–4.72 · 8.0–8.5 | **2.28–2.52 · 3.41–3.60** |
| Headless Chromium `e2e/chase_perf.mjs` (23 vehicles): sim ms per 60 Hz frame | — | 10.4–11.3 | **5.5–6.3** |
| Same, sim ms per rendered frame | (browser-perf: 65–76) | 147–159 | **76–89** |
| `e2e/perf_check.mjs` `sim` section, chase · rain · phone, ms per rendered frame | (browser-perf: 65–76) | 121–138 · 132–139 · 109–119 | **72–86 · 78–80 · 61–64** |
| `perf_check` real-time factor, chase · rain · phone | (browser-perf's box: 0.85–0.95) | 0.78–0.82 · 0.69–0.81 · 0.81–0.87 | **0.77–0.83 · 0.73–0.81 · 0.80–0.90** |

**Read the RTF row with the machine calibration in `browser-perf.md`.** On this container the
frame is set by SwiftShader raster and compositing (~180 ms of a 250–330 ms frame, same for both
builds). Halving the sim section moved the real-time factor only 0–0.05, so neither build reaches
0.9 here. The sim's share is back to its pre-merge size: 0.25–0.3 s of CPU per wall second,
was ~0.5. On the perf_check machine, where the pre-12b build reached 0.92+, the sim is a larger
part of the frame. Expect it back near that, but **the ≥ 0.9 target was not verified on that
machine from here**.

Native results are back to the pre-merge cost (chase 1.45–1.73 vs 1.54–1.60 ms) and well within
the 0.15 ms/car budget everywhere (≤ 0.077 ms/car city, ≤ 0.066 flat). The browser budget is 2×.

Noise: this container is shared. The same binary moved ±15 % between runs. Every A/B above is
interleaved on the same machine within minutes. Ranges are 2–3 runs.

## 1. Measurement: the shape-cast cause, confirmed

Callgrind, 10 s of the `chase_bench` city chase (instructions, inclusive; the runs diverge, so
damage / contact costs differ between them):

| | Before | `lead/round12` | This branch |
|---|---|---|---|
| `Sim::step_fixed` | 4.93 G | 7.81 G | **3.33 G** |
| `Vehicle::step_with_ground` | 1.92 G | 5.83 G | **1.65 G** |
| … wheel probes (`cast_wheel`) | 0.89 G (ray) | 4.97 G | **0.80 G** (ray + sweep) |
| … … Rapier GJK `cast_shape` (cylinder sweep) | — | **4.06 G**, 1.71 G of it vs the terrain heightfield | 0 in the city |
| … … closed-form `sweep_triangle` | — | — | 0.27 G |
| … tyre model (`mf_shape`) | 0.19 G | 0.19 G | 0.19 G |
| Rapier step (contacts, solver) | 0.72 G | 0.70 G | 0.75 G |
| police | 0.45 G | 0.45 G | 0.45 G |
| replay hashes (`state_hash` + `entity_hashes`, per tick) | 1.96 G | 1.96 G | 1.96 G |
| `wake_resting_props` (scan of ~3800 parked props) | 0.28 G | 0.28 G | **0.00 G** |
| lattice / damage (`apply_contacts`) | 1.05 G | (fewer contacts in this run) | (fewer contacts) |

The step grew by 2.9 G and the GJK cylinder cast alone was 4.06 G, so **the cause is confirmed**.
Rapier's composite cast runs GJK (`gjk::directional_distance`, `Cylinder::support_point`,
Voronoi simplex / tetrahedron projection) against every road, sidewalk and kerb triangle and
every terrain heightfield cell in the swept box, four wheels per car per 240 Hz tick. That is
~7 µs per wheel in Port Vesper, against 2.5 µs on the flat rig and 0.57 µs for the old ray. The
strut solve and the rest of the 12b model add ~0.15 G (≈ 8 %), not the regression.

In the browser (`e2e/cpu_profile.mjs`, round 12): `cast_wheel` 1523 ms of `step_fixed` 2770 ms
over 8 s (55 %), `cast_shape` 1269 ms. With this branch: `probe` 253 ms of 1800 ms (14 %).

## 2. The fix (driving feel unchanged)

**Closed-form tyre sweep against triangles** (`RapierGround::probe`,
`TyreFrame::sweep_triangle`). The 12b design is kept as it was: a centre ray blended with an
axle-aligned cylinder sweep (`blend_envelope`, same thresholds, same 0.3 m lift, same 80 % width,
static world only). Only the way the sweep's first touch is computed changed:

- A point `P` at side `s`, across `u`, along `l` (tyre frame) is first reached by the swept
  tyre at travel `t = l − √(r² − u²)` if `|s| ≤ half_width`, `|u| < r`. This is the same
  circle-through-the-witness solve `sweep_hit_on_tyre` already applied to GJK's witness.
- The touch on a triangle is the minimum of `t` over the triangle clipped to `|s| ≤ hw`,
  `|u| ≤ r`. `t` is convex and depends on the point only through `l` and `u`, so the minimum is
  on the polygon boundary: a closed-form 1-D minimisation per edge (≤ 7 edges).
- Overlap at the start (GJK "penetrating" → sweep void, the ray is kept, as before): some point
  has first-touch ≤ 0 ≤ last-touch.
- Normal: the face normal for a touch inside the face, the tyre's radial direction for a touch
  on a triangle edge or corner. These are what GJK reports for those features.

**One pass, ray included.** The colliders whose box meets the swept tyre's box are visited
once (`intersect_aabb_conservative` with the same static-only filter). For trimeshes and
heightfields, each triangle in the box gets parry's own `Triangle` ray test, which is the call
parry's mesh and heightfield ray casts make, and the closed-form sweep. Every other shape (the
flat-ground slab, barrier and kerb boxes, building hulls, rails, tunnel walls) gets exactly the
per-collider `cast_ray_and_get_normal` and `cast_shapes` calls Rapier's query pipeline made.

**Accuracy** (`probe_cost`, 20 000 wheel positions on Port Vesper lanes, centre and both lane
edges, so kerbs and junction steps are included): the sweep lifts the wheel above the ray
(> 1.5 mm, where it matters) on 34 % of them. Closed form vs GJK: 19 999 of 20 000 both hit,
**mean |Δ| 0.011 mm**, 2 over 1 mm (max 4 mm). The one GJK-only miss is a GJK witness 0.72 m
from the tyre that `sweep_hit_on_tyre` rejects. The resulting wheel travel differs by a mean of
0.005 mm. The centre ray is identical on 19 973 of 20 000. The other 27 are exact ties on a
shared triangle edge where the neighbouring triangle (same distance, its own normal) wins.
Unit tests (`ground.rs`):
`triangle_sweep_matches_gjk` (4000 random tyres × triangles: same verdict, ≤ 0.5 mm from GJK;
the 23 cases further off are settled against an independent brute-force slicer, within 0.2 mm,
and in all of them GJK is the one that is off) and `triangle_sweep_on_flat_ground`.

**Flat ground is unchanged bit for bit**: every flat-world golden matched after the change,
before any bless.

**Resting props** (`Sim::wake_resting_props`): each tick read the body of every resting parked
car (~3800 in Port Vesper) to find those knocked into motion. That was 0.28 G per 10 s natively
and ~200 ms of 1800 in the browser profile. Only an awake body can be moving, so the tick now
checks Rapier's awake set against a set of resting bodies and runs the old scan only on a tick
where one of them did move. Result identical (all 82 goldens matched after this commit).

### Options from the brief, measured or bounded

| Option | Verdict |
|---|---|
| Cheaper probe shape (ray fan, capsule / sphere cast) | Not needed, and worse. A Rapier ball or capsule cast is the same GJK per triangle (the support map is not the cost), and changes the kerb envelope. A ray fan costs 0.57 µs per ray, so a 5-ray fan ≈ 2.9 µs, more than the exact sweep (1.31 µs incl. the ray), and it is only an approximation. |
| Static-only, broadphase-filtered query set | Already so since 12b (`EXCLUDE_DYNAMIC \| EXCLUDE_KINEMATIC`, own body excluded). The new pass keeps the same filter (`intersect_aabb_conservative` applies it). Other cars, debris and woken props stay out. Not where the cost was. |
| Reusing last tick's hit (wheel moved < ε) | Not done. At 15 m/s a wheel moves 6 cm per tick, so the cache would only hit for stopped cars. The whole remaining sweep cost (1.31 − 0.57 µs) × 16 per car-frame bounds the saving at 0.012 ms/car/frame, for a new invalidation rule in the hash. |
| Lower probe frequency far from contact | Probes already run once per 240 Hz tick, not per vehicle sub-step. Same bound as above. A wheel in the air needs its probe most (landing). |
| LOD: far AI / traffic cars use a ray | Not done. Same 0.012 ms/car/frame bound, and it would make physics depend on the camera (the owner rule: one physics for everyone). |
| Closed form for boxes too (the flat slab) | Possible follow-up: flat ground still pays ~2.5 µs per wheel of GJK against the 10 km slab (0.055–0.065 ms/car/frame vs 0.027 pre-merge, within budget). It would move every flat golden, so it was left out of this pass. A half-space slab (A's suggestion) is the other route. |

## 3. Determinism, goldens, scenarios

- **Goldens:** 26 city goldens + the `golden_city_chase` replay moved in one `[bless]` commit.
  Reason: contact geometry moves by GJK's own error (sub-0.1 mm) and exact edge ties pick the
  other triangle; city runs then diverge chaotically. Moved: `city_*` (8), `golden_city_chase`,
  `opponent_*` (4), `police_sideswipe_patrol_night`, `police_tunnel_escape_founders_cut[_control]`,
  `race_*` city races (5), `world_traffic_soak_observer`, `world_traffic_soak_vesper[_observer]`,
  `weather_fog_pursuit[_clear]`. Every flat-world golden, 56 of them, is unchanged. The
  resting-props commit moved none.
- **Scenarios** (`simcli run apps/simcli/scenarios`): all 82 pass on this branch, as on
  `lead/round12`. That includes `vehicle/kerb_strike_{30,60,60_angled}`, `city/sidewalk_mount_*`,
  `city/hill_road_*` and `city/loop_crest_at_limit`. The pending lists are the same, except one
  pending contact-count check that now passes. `wp_vehicle` `tests/ride.rs` and
  `tests/handling_balance.rs` run on flat ground and are untouched by construction (gate below).
- **Native == wasm** (`tools/wasm-determinism/compare.sh` on `city/`, `vehicle/`, `golden/`,
  `race/vesper_hollins_sprint`, `police/sideswipe_patrol_night`): 34 / 34 identical.
- **Debug == release:** see the gates table.
- Only IEEE `+ − × ÷ √`, min/max and clamps in the new code. Visiting order is the BVH's
  (fixed). Ties keep the first found.

## Changes outside my area

- `crates/wp_vehicle/src/ground.rs` (A's crate, probe path only): `RapierGround::cast_wheel` →
  `probe`, `TyreFrame`, `TriSweep`, `sweep_tyre`, `sweep_tyre_gjk` (the old sweep, kept as the
  reference for tests and the bench). No change to `vehicle.rs`, the probe geometry, blend
  constants, steering, assists or data. Merges with A's steering / TCS work should be clean.
- `apps/simcli/examples/vehicle_cost.rs` (I's crate, new example only), `apps/simcli/golden/*`
  (`[bless]`).

## Requests

- **Lead:** run `e2e/perf_check.mjs` on the perf_check machine to confirm RTF ≥ 0.9 there. This
  container is raster-bound (see above). After this branch the largest removable sim cost in the
  browser is the per-tick replay hash (`state_hash` + `entity_hashes`: 1.96 G per 10 s natively,
  ~410 ms of 1800 in the browser profile), as browser-perf already noted (hash spec v4).
- **A (vehicle):** nothing to change. If you touch the probe again, `wp_sim --example
  probe_cost` gives cost and accuracy against the GJK reference in one run.
- **Testbed / harness:** `e2e/chase_perf.mjs` and `e2e/cpu_profile.mjs` kill `npx` but not the
  Vite it starts. The server stays on its port, and the next run in another checkout silently
  measures the old build. (`frame_profile.mjs` kills its process group correctly.)

## Gates (2026-10-20, cloud container, `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`)

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo test --workspace --no-fail-fast` | see below |
| `simcli run apps/simcli/scenarios` | 82 / 82 pass |
| `simcli goldens` | 82 / 82 after the `[bless]` |
| native vs wasm (`tools/wasm-determinism/compare.sh`, subset above) | 34 / 34 |
| `cargo build --target wasm32-unknown-unknown -p wp_wasm` | pass |
| `npx tsc --noEmit` | pass |
| `e2e` | see below |
