# Wave 2 — style-traffic

Owner files: `tools/assets/src/styles/traffic.rs`, `apps/testbed-web/src/render/traffic.ts`, minimal
hook in `apps/testbed-web/src/scene.ts` (`updateAgents`, `render`, `setTimeOfDay`), the six traffic
meshes in `data/panels/meshes/`. Spec: `research_traffic.md`.

## A. Civilian bodies (`styles/traffic.rs`)

Each car = a `BodyStyle` from its silhouette preset with the research §3 knobs, plus one custom
detail function (its "trim kit") built from small helpers that read the styled `BodyProfile`:

- `nose_y` / `tail_y` (front / rear face y at a lateral offset, following the plan rounding),
  `front_box` / `rear_box` / `side_box` (boxes whose inner face follows the surface),
  `side_strip` (thin shell along the side, split at panel seams and skipping wheel arches, each
  piece skinned to its own panel), `reshape_pair` (replaces the generic head / tail light boxes
  with era-appropriate shapes, keeping their part names / regions).
- Pieces: framed grilles with slats, bumper bars with corner wraps and chrome strips, plates in
  dark recesses, door handles (one per door, crew-cab rear handle on the hauler), amber / red side
  markers, wipers, chrome belt trim, rub strips, drip rails, black / tall two-arm mirrors (generic
  paint mirrors hidden), third brake lamp, shark-fin and whip antennas, hatch spoiler + rear wiper,
  rocker cladding, bed-rail caps, tailgate handle, step bumpers with hitch + ball, mud flaps,
  running boards, red tow hooks (caliper slot), cab clearance lamps, van barn-door seam / hinges /
  handle, roof marker lamps, sliding-door track + slider handle (right side only).
- Wheels / mech: sedan OEM alloy, beater + supermini steel + full hubcap (4 lugs, rear drums),
  compact pickup steel + centre cap (leaf rear, drums), hauler chrome truck wheel (6 lugs,
  14 segments for budget), van steel + small cap (8 lugs).

| car | tris (total, incl. wheels / mech) | body + details |
|---|---|---|
| debug_sedan | 8 422 | ≈ 5 180 |
| junkyard_beater | 7 960 | ≈ 5 290 |
| tansy_13 | 7 192 | ≈ 4 560 |
| furrow_24 | 8 490 | ≈ 5 450 |
| hauler_1500 | 9 781 | ≈ 6 440 |
| drayman_cargo | 9 435 | ≈ 6 220 |

All within the 4k–10k traffic budget (hauler has the least headroom: ~220 tris).

## B. Instanced traffic (`render/traffic.ts`)

- `TrafficRenderer` (owned by `SceneView.traffic`, group under `world`). `scene.updateAgents`
  calls `traffic.update(agents, stride, skipPhysical, now)` once the meshes are loaded and keeps
  the old grey-box `InstancedMesh` as the fallback (meshes missing / still loading). The old path
  no longer allocates per agent.
- Per model: the panel mesh merged into one static geometry (interior, engine, suspension, discs,
  calipers, uprights dropped; wheels translated to `data/cars/<id>.json` hub positions), one
  `InstancedMesh` near LOD + one far LOD hull (≈ 300 tris: 10 stations from slice bounds, glass
  band, lamp boxes, 8-sided wheels) beyond 55 m; not drawn beyond 700 m or behind the camera.
- One shader for everything (`MeshStandardMaterial` + `onBeforeCompile`): materials baked into
  vertex `color` + `aSurf` (paint group, lamp kind, roughness, metalness); per instance
  `instanceColor` (paint), `iAlt` (alt paint + mask + livery kind), `iParams` (rust, dirt, lamp
  bits, text row), `iPbr`. Procedural taxi checker band, delivery band, word-atlas lettering
  (canvas, invented words only), rust blotches low on the body, dirt toward the sills. Lamps:
  head / tail glow at night, brake lights from the agent flag, indicators / hazards blink,
  taxi sign and work beacon emissive.
- Props (one `InstancedMesh` each, placed on roof / bed anchors measured from the mesh): taxi
  sign, roof rails, roof box, pickup ladder rack + ladder, cross-bed toolbox, amber beacon,
  tonneau cover, van roof rack + ladder.
- Looks: `trafficLook(carId, variant, id)` (exported, pure) picks a research §4 variant per model
  and a paint from the §5 tables (civilian, superminis ×2 accents, pickups grey-scale, vans ≥ 75 %
  white, work trucks, fleet, ex-fleet, taxi yellow, faded beater + primer panels) with ±2 % value
  jitter. Variants: sedan S1 / S2 taxi / S4 fleet / S5 roof box + rails / S6 ex-fleet; beater B1 /
  B2 mismatched panels / B3 rusty; supermini H1 / H2 rails / H3 delivery roof box; compact pickup
  P1 / rusty / P2 work; hauler F1 / F4 tonneau / P2 work; van V1 / V2 delivery band + lettering /
  V3 trade lettering + roof rack.
- `trafficPaint(carId, variant, id)` → `{ color, metalness, roughness }` for promoted cars (not
  wired; see requests).
- Model per agent: stride 7 + `traffic_models()` with more than one id → the sim's model (kept on
  promotion). With 0–1 sim models (free drive, today's city chase) every agent would look the
  same, so the renderer picks a body from the agent id with the research §2 weights. Old wasm
  builds (stride 5, no ids): `trafficStride()` detects them; a nearest-previous-position tracker
  keeps a stable per-agent seed.

### Perf (headless Chromium + SwiftShader, cloud VM)

| case | `TrafficRenderer.update()` CPU | draw calls | triangles |
|---|---|---|---|
| 400 synthetic agents (20 × 20 grid, 18 m apart, 6 models, 1/3 braking) | **0.06–0.15 ms** direct (300 calls), 0.2 ms median / 0.3 ms p90 in the frame loop | 19 (all traffic + props) | 144 k (3 near, 397 far) |
| Port Vesper free drive (34 agents) / city chase (17 agents) | 0.1 ms median, 0.2–0.3 ms p90 | — | — |

Well inside the 2 ms browser target. Every instance attribute is rewritten each frame (cheap at
this count, ~29 floats per agent) with `addUpdateRange`. GPU worst case: all 400 agents within
55 m would be ≈ 400 × 6–7 k = 2.6 M triangles; lower `LOD_NEAR_M` if a dense jam ever shows up.

## Screenshots
`/tmp/claude-0/-home-user/1061d6c1-01ae-51aa-aaa0-b18f6899236a/scratchpad/traffic_*.png`
(each car front / rear via the instanced renderer, variant line-ups day / night, 400-agent
bench, Port Vesper street filled with synthetic agents day / night).

## Requests for the lead
1. **Promoted traffic colour**: in `main.ts` (vehicle creation for `VEHICLE_KIND.traffic`) use
   `trafficPaint(info.car_id, (metaFlags >> 8) & 0xff, agentId).color` instead of `0x7a8a99`, and
   the metalness / roughness on `PanelBody.paint`. Needs the agent id of the promoted car (e.g.
   from the entity name `traffic<id>` if that is the agent id, or a `vehicle_meta` field).
   Props / liveries are not reproduced on the physical car (instanced-only).
2. **More sim models in sessions** (`set_traffic_cars` with all six) so the sim, not the
   renderer, picks bodies; then promotion keeps the body as well as the colour.
3. Traffic density in the testbed city sessions is low (17–34 agents); the 400-agent number above
   is a synthetic bench.
4. Shared `node_modules` was a symlink into a deleted worktree; I installed a copy at
   `/home/user/.node-testbed/node_modules` (also mentioned by the lead). A `wasm-bindgen` 0.2.129
   CLI is at `/home/user/.wbcli/bin/wasm-bindgen`.
