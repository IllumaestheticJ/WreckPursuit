# Painted-car pipeline integration notes

- **Workstream / prompt:** car overhaul — modelling / pipeline agent (first family: Nissan
  Skyline R34)
- **Branch:** worktree branch `worktree-agent-a65b475049e1ca0df` (merged
  `claude/adoring-ramanujan-2jmlj6`, the stats agent's R34 cars / lattices / panels)
- **Crates / folders owned:** `tools/assets` (`section.rs`, `paint/`, `styles/models/`, hooks in
  `profile.rs` / `mesh.rs` / `style.rs` / `wheels/` / `mechanical/spec.rs` / `validate.rs`,
  `mesh_shots.mjs`), `data/panels/models/`, `data/panels/textures/`, the R34 meshes in
  `data/panels/meshes/`
- **Contract version built against:** current `main` (`wp_contracts` untouched)

Plan followed: `pipeline_plan.md` steps 2–7 (section spec + adaptive loft, `Tier::Painted` +
model style, `duvs` + mesh format 5, painter, validate / tests, `mesh_shots.mjs` preview).
Step 8 (game renderer) is the next step, see "Needs".

## Exposes
- **Mesh format 5** (`data/panels/README.md` "Format 5: painted cars"): `CarMesh.detail =
  { file, w, h, hash }` (texture `data/panels/textures/<car>.png`, RGBA8, 1024²) and per-part
  `duvs` (second UV set, 1/65535, v down from the PNG's first row). Only painted meshes are
  format 5; all other meshes are format 4 and byte-identical to `main` (`gen-car-mesh --check`).
- **Shader rule**: `albedo = tex.rgb · mix(1, paint, tex.a)` (A = paint mask, RGB = baked
  shading on paint, colour elsewhere); lamps `emissive = tex.rgb · level`; police livery stays
  `paint = liveryMap(uv0)`.
- **`car_assets::section::SectionSpec`** — family file `data/panels/models/<family>.json`;
  `BodyStyle::sections` (serde-skipped) drives `BodyProfile`.
- **`style_for`** asks `styles::models` first (`Tier::Painted`, budget 4k–30k). Ids:
  `nissan_skyline_r34_{25gt,25gt_t,gtr,gtr_vspec2}`.
- `car_assets::{build_detail_png, detail_path}`, `mesh::{painted_detail, is_painted,
  MESH_FORMAT (5), MESH_FORMAT_PLAIN (4)}`, `validate::{validate_detail, validate_detail_file}`.
- `mesh_shots.mjs`: `SHOT_ORTHO`, `SHOT_REF`, `SHOT_CAMS`, `SHOT_FOV`, `SHOT_BG`, `SHOT_NOTEX`,
  `SHOT_FLAT`, `THREE_DIR`, `PLAYWRIGHT_CORE`.

## Needs
1. **Game renderer (plan step 8, not done this round)**: `apps/testbed-web/src/render/detail.ts`
   (`loadDetailTexture(mesh)` → `THREE.Texture` with `flipY = false`, `channel = 1`, nearest mag
   filter + mipmaps; `applyDetailMap(material, slot)` via `onBeforeCompile` replacing
   `#include <map_fragment>` with the rule above and `customProgramCacheKey` = one program for
   all painted cars); `panels.ts` (`dataOf` reads `duvs` → `uv1`, batch templates and crease
   layout carry `uv1`, `debrisGeometry` copies it, per-model material copies so one car's
   texture does not leak to another, `setLivery` keeps `uv0`); `fleet.ts` (merged geometry
   outputs `uv1`, per-model `FleetMaterials` clone); `crease.ts` carry `uv1`; optional preload in
   `lod.ts`. Without these the R34s draw with plain material colours in the game (correct shape,
   no painted detail). Not attempted here because the testbed's npm dependencies (three,
   @types/three, vite) and the wasm build are not installed in this environment, so the TS could
   not be type-checked or run in e2e.
2. **Stats agent / lattice (data request)**: the generated coupe lattice puts the R34 front door
   hinge at y = 0.388 m and its rear edge at −0.605 m; the real door runs ≈ +0.95 → −0.35 m
   (its front cut line is right behind the front wheel arch, under the A-pillar foot). The mesh
   keeps the lattice zones (rule), so the painted door shut line sits ≈ 0.55 m too far back and
   the front fender is too long. A `gen-lattice` door-station knob (or a per-car override) would
   fix it; the hood hinge (1.004 m) and trunk hinge are right.
3. Per-car tables (catalog, lots, capability, street, stats, audio, events, AI) are the stats
   agent's; not touched here.

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| – | none (`wp_contracts` unchanged) | | | |

## Changes outside my crates
- `tools/assets/src/style.rs` (lead-owned file, owner-directed): `BodyStyle::sections`,
  `WheelStyle::painted`, `Tier::Painted`, `style_for` asks `styles::models` first.
- `crates/wp_render/src/car_mesh.rs`: `MESH_FORMAT_MAX` 4 → 5 so format-5 meshes load (the
  reader ignores `detail` / `duvs`).
- Root `Cargo.toml`: `[profile.dev.package.car-assets] opt-level = 2` (the AO bake runs in
  `cargo test -p car-assets`). `tools/assets/Cargo.toml`: `miniz_oxide = "0.9"` (already in the
  lock file via `wp_wasm`; pure Rust; car-assets is not reachable from `wp_wasm`).
- `tools/assets/src/mechanical/spec.rs`: table rows for the four R34 ids; `hero` detail for
  `Tier::Painted` (`tier != Traffic`; Hero / Traffic cars unchanged).

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| Paint one 1024² texture (raster + AO 16 rays on a 2-texel grid + features + PNG) | cloud 4 vCPU, dev profile opt-level 2 | ≈ 1 s | – | build time only |
| `gen-car-mesh nissan_skyline_r34_gtr` (full + mid + texture) | same | 2.2 s | – | – |
| R34 GT-R triangles | – | full 15 161, mid 5 167 | – | Painted 4k–30k, mid 2k–7k |
| Texture | – | 1024² RGBA8, ≈ 380 kB PNG | one texture per car, shared by both LODs | ≤ 2048² |

Frame cost in the game: not measurable until step 8 (one extra texture fetch per painted
fragment, one shader variant).

## Known gaps
- Game renderer (above). Painted cars look right only in `mesh_shots.mjs` for now.
- Door / fender shut lines follow the lattice (above).
- R34 proportions: anchors and curves fitted to reference photos (front / rear 3/4, rear,
  wheel close-up) and the published dimensions; no orthographic blueprint was reachable, so
  side-view heights and stations are ±2–3 cm estimates. 25GT / 25GT-T heights use the GT-R's
  roof (their 1340 mm height is 2 cm lower in reality).
- Painted detail is drawn per view (front / rear / side / top projections): features on
  surfaces that face between two views (nose corners) switch projection at the 45° line.
- The exhaust pipe run under the rear bumper is visible from low rear views (mechanical
  exhaust layout; the R34's real muffler sits higher behind the valance).
- Reference images used for fitting stay local (scratchpad), never in the repo.

## Lead feedback
(Written by the integration lead.)
