# PROMPT 11 — Art direction options, asset pipeline, and the greybox → low-poly transition (later phase; `docs/art/`, `tools/assets/`, `apps/testbed-web` materials)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `00_README_OVERVIEW.md` §5. You own `docs/art/`, `tools/assets/`, and shader/material code under `apps/testbed-web/src/render/`. Branch `ws/art`. Do not start before Checkpoint 2 (greybox city + full loop) exists on `main`.

## Goal
Take the prototype from debug boxes to **Checkpoint 3: low-poly playable in a dark, gritty palette** without blocking physics work, and prepare the pipeline so deformable, detachable, modifiable car meshes are cheap to author.

## Work
**Art bible draft** (`docs/art/ART_BIBLE.md`): present the four options (Neon-noir realism, Stylized low-poly dark, PS2/early-HD throwback, Graphic-novel halftone) with mood descriptions, time-of-day palettes (dawn/day/dusk/night — night must stay readable; police lights are the brightest thing in the frame), UI tone (scanner-green monospace + grime vs. clean sim HUD), and a recommendation. Include a checklist for how each style treats damage readability (dents must read at 30 m in chase cam).

**Car mesh pipeline** (`tools/assets/`): 
- Mesh spec: 3–6k tris low-poly body, separate meshes per detachable region matching `BodyRegion` names, glass separate, wheels separate, interior shell minimal; consistent pivot/scale (metres, +Y forward, +Z up); UV layout reserving a 0–0.5 U band for livery/vinyls later.
- `bind-skin`: given a car GLB + its `LatticeDef`, computes `skin_binding` (4 nearest nodes, inverse-distance weights, region-aware so hood verts don't bind to fender nodes) and writes it back into the lattice JSON. Also emits per-region convex hulls used for detached debris.
- `validate-car-asset`: checks naming, pivots, tri budget, that every region in the lattice has a mesh, that mod body-kit parts attach to the right sockets.
- Placeholder generation: a procedural low-poly car generator from `VehicleDef` dims + body profile (box-with-chamfers, greenhouse, wheel arches) so **every car in the roster has a mesh immediately** that already deforms correctly. Hand-made meshes replace these one by one.

**World materials**: triplanar asphalt with wetness parameter (ties to weather later), emissive signage for Core at night, cheap street lights (instanced), police/traffic light bars with correct flash patterns, sodium vs LED street-light colour by district, fog/particulate volume, skid marks & sparks (decal pool), debris persistence rules.

**Post stack** (Three.js): tonemap, filmic grain, slight chromatic aberration at speed, vignette, optional halftone pass (for option D evaluation). All toggles in the debug menu so leadership can A/B styles in the same build.

**UI kit**: HUD and garage components in the chosen style; damage silhouette; scanner feed; captain map theme.

**Deliverables**: `ART_BIBLE.md`, pipeline tools + docs, procedural meshes for all 13 cars, A/B-able post stack, screenshots per style in `docs/media/`, and a `docs/art/ENGINE_DECISION.md` input: what the final target engine must support (deformable skinning path, decals, instancing counts, audio middleware) to drive the Godot-vs-Unreal choice at Checkpoint 3.
