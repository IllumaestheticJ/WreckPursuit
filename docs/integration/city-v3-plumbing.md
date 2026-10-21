# Port Vesper v3 — feature plumbing (colliders, greybox, wasm, testbed)

- **Workstream:** city v3 plumbing. **Branch:** `v3/plumbing`.
- **Areas:** `crates/wp_sim/src/physics.rs` (feature colliders), `tools/city-import/src/glb.rs`,
  `crates/wp_wasm/src/features.rs` + exports, `apps/testbed-web/src/features_view.ts` + wiring,
  new `crates/wp_world/src/palette.rs` (one line in `wp_world/src/lib.rs`).
- **Contract version built against:** 0.3.0 (no `wp_contracts` change).

## What every feature generator gets for free
| Shape | Physics (`solid`) | Rendering |
|---|---|---|
| `Box` | cuboid, rotation `q = q_yaw · q_pitch` (pitch about local +X first, + = +Y end up; then yaw about +Z, 0 = north, CCW) | testbed, instanced (`city_features()`) |
| `Cylinder` | Rapier cylinder turned onto +Z, centre = base + height / 2 | testbed, instanced, 10-sided |
| `Prism` | convex hull of footprint × `[base_z, top_z]` | greybox, `feature_<material>` primitive |

- Colliders are grouped per 256 m tile (by shape centre) and drivability into compound
  colliders, inserted after all pre-v4 colliders (other cities unchanged).
  Drivable kinds (friction 0.8): `ParkingDeck`, `ParkingRamp`, `Runway`, `Taxiway`, `Apron`,
  `Pier`, `Platform`, `Earthwork`, `Tunnel`, `RailBridge`, `Ground`; others 0.5.
- `metro::ctx::ramp(a, b, ..)` and the collider agree: the top centreline passes through `a`
  and `b` (unit test in `physics.rs`); a car climbs a 1:7 ramp onto a 6 m deck
  (`crates/wp_sim/tests/features.rs`).
- Colours: `wp_world::palette::material_rgba` / `material_name` (greybox, wasm
  `feature_palette()`, testbed). `CarPaint` gets a per-instance colour, `Light` is emissive.

## Rules for feature authors
- **Solid prisms must be convex** (the collider is the hull): split concave shapes.
- Degenerate shapes (a half extent ≤ 1 mm, < 3 footprint points, top ≤ base) get no collider.
- Flat paint / lawn prisms sitting on the ground: lift them 2–5 cm (the testbed polygon-offsets
  `feature_paint_white`, `feature_paint_yellow`, `feature_grass`, but z-fighting with terrain
  is still possible at range).
- Budget: 50 k solid boxes cost 57 ms to build and ~5 µs per physics step for a car among them
  (native release); rendering is ~one draw call per (512 m chunk, shape, material) within 1.5 km.
  Keep box + cylinder features under ~200 k (wasm memory: 44 bytes each in the export plus a
  64-byte instance matrix) and prism vertices modest (they land in the greybox glb).

## Greybox size
Terrain over 25 km² (only v3) draws 2 × 2 render-cell blocks with no road within 60 m as one
quad (crack-free). v3 with today's stub roads: 15.3 MB → 6.6 MB glb (8.8 MB base64). The full
road network keeps more terrain at full resolution (upper bound ≈ 15 MB of terrain); buildings and
prisms add to that. v2 / grid greyboxes are byte-identical (stale tests pass).

## Testbed
`?city=vesper_v3` / the selector option (v2 stays the default). The city is generated in the
browser (~5 s). A missing greybox only logs a warning. `node e2e/features_shots.mjs [outDir]`
screenshots v3 with a synthetic feature set when the city has none yet.

## Contract change requests
None.

## Known gaps
- Features have no LOD beyond chunk draw distance; no shadows / textures.
- Compound colliders per tile: one contact event per car-tile pair per tick (all sub-shapes
  touched in that tile are summed), which is what damage needs, but event counts differ from a
  one-collider-per-feature layout.
