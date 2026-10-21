# data/panels — panel specs and car meshes (workstream R, prompt 21)

Owned by workstream R (`ws/panels`). Every car in `data/cars` has:

| File | What | Made by |
|---|---|---|
| `<car>.json` | Panel spec (`wp_damage::PanelSpec`): hinged/bolted panels, hinge + latch nodes, swing limits, mass, inertia, damping, thresholds | `cargo run -p car-assets --bin gen-panels` (first pass from the lattice; hand edits welcome) |
| `meshes/<car>.json` | Procedural low-poly mesh (`car_assets::mesh::CarMesh`), one part per body region + glass, lights, mirrors, interior, wheels; skinned to the lattice | `cargo run -p car-assets --bin gen-car-mesh` (runs `bind-skin`) |
| `meshes/mid/<car>.json` | Mid level of detail of the same car (same format and fields, `car_assets::mesh::Lod::Mid`), not skinned; see "Mid LOD" below | `cargo run -p car-assets --bin gen-car-mesh` (same run) |

Both are checked by `cargo run -p car-assets --bin validate-car-asset` and by
`cargo test -p car-assets` (which also fails if `meshes/` or `meshes/mid/` is stale, like the
lattices test).

### Mid LOD (`meshes/mid/<car>.json`)
A second file per car in the same format 4 / `CarMesh` shape, in a subfolder so anything listing
`meshes/*.json` still sees one (full) mesh per car. Generated from the car's own style
(`mesh::generate_mid`): loft stations every 0.20 m / 0.12 m near the ends and arches
(`MID_LOFT_STEPS`; full meshes 0.07 / 0.036 hero, 0.11 / 0.065 traffic), 12-segment wheels,
16 cm surface-patch grids, traffic-tier mechanical detail, and only the parts that read from outside at a distance
(`mesh::keep_in_mid`): body panels, glass, lights, mirrors, light bars, tires, rims, exhaust tips
and style details ≥ 0.2 m across. Interior, engine bay, suspension / exhaust runs, brake discs,
calipers, uprights, decal carriers, underglow and small details (handles, fuel fillers, mirror
glass …) are dropped; details, rims, lamps, light bars and mirrors are decimated by vertex
clustering on a 3.5 cm grid. `id`, `lattice_id`, `atlas`, `livery`, `tints` and `wheel_camber_deg` equal
the full mesh's, paint / livery parts keep their livery-atlas UVs, so paint, police liveries and
tints look identical. Part names, kinds and materials are a subset of the full mesh's. No
`skin_nodes` / `skin_weights` / `fold`: it is only drawn undeformed. 2k–6.5k triangles
(`MID_TRI_BUDGET`, test `every_car_has_an_up_to_date_mid_lod`).

Used by the browser testbed (`render/panels.ts` `loadCarMidMesh`, falling back to the full mesh
when the file is missing) for the instanced fleet's near tier and the nearest parked cars;
promoted cars within ~30–40 m, the player and the AI racers draw the full, skinned mesh (their
own `VehicleView`), so dents and crumple show where they can be seen.
Analog names only, no badges or trademarks (`tools/specsheet/nameplate_check.py`).

## Panel spec (`<car>.json`)

```jsonc
{
  "id": "debug_sedan",            // car id == file stem
  "lattice_id": "debug_sedan",    // VehicleDef::lattice_id; node ids refer to it
  "panels": [{
    "name": "door_fl",            // unique per car
    "region": "LeftDoorF",        // BodyRegion; every free lattice node of it belongs to the panel
    "kind": "Door",               // Door | Hood | Trunk (hinged: unlatch → swing) | Bumper | Fender (bolted: hang)
    "group": 4,                   // lattice Detachable::group; null → synthetic group (needs break_impulse_ns)
    "hinge_nodes": [159, 163],    // hinge line (bolted kinds: the two end mounts)
    "latch_nodes": [144, 148],    // their mean displacement past latch_intrusion_m pops the latch
    "open_dir": [-1, 0, 0],       // which way it opens (+angle)
    "max_angle_rad": 1.2,         // swing stop of a hinged panel
    "hang_axis": [1, 0, 0],       // pivot axis when hanging from one node
    "hang_limit_rad": 0.5,        // ± swing while hanging (bumpers: until the far end meets the road)
    "mass_kg": 22.0,
    "lever_m": 0.56,              // hinge → centre of mass of the real panel
    "gyration_m": 0.65,           // I = mass · gyration²
    "area_m2": 0.45,              // aero area when open
    "damping_per_s": 0.6,
    "restitution": 0.3,           // stop bounce
    "unlatch_impulse_ns": 2000.0, // group hinge impulse that pops the latch
    "latch_intrusion_m": 0.06,
    "hinge_break_impulse_ns": 440.0, // stop-slam load: one hinge fails at this, the second at 2×
    "break_impulse_ns": null      // synthetic groups only
  }]
}
```

All impulse thresholds are multiplied by the mode's `beam_break_scale` (Arcade keeps panels on
longer). Generated values: doors/hood/trunk from the lattice detachables, bumpers hang from their
end mounts, fenders are synthetic groups (≈ ⅓ door mass) hanging from their A-pillar edge. Rear
doors exist on 4-door lattices only; hatch/pickup tailgates whose lattice nodes all sit on the
hinge line are skipped (they stay plain detachables). Mirrors have no lattice region yet (they are
part of the front-door mesh; see `docs/integration/panels.md`, contract request R-1).

## Mesh (`meshes/<car>.json`), format 4

Format 4 is the union of two independent extensions of format 1: `claude/detailed-car-models`
called its extension "format 2", `lead/round12` went to "format 3" (its own earlier format 2 had
`fold_chords` / `fold_weights`, now gone). The field names and JSON shapes below are unchanged
from each side; only the number is new:

| Field | From |
|---|---|
| `id`, `lattice_id`, `format`, `tri_count`, `parts[]` with `name region kind material wheel positions_mm uvs indices skin_nodes skin_weights` | format 1 |
| `tints`, `livery`, `atlas`, `wheel_camber_deg`; part `rigid`; new `kind` / `material` values; livery-atlas UVs on paint parts | `claude/detailed-car-models` (its "format 2") |
| part `fold` = `{ chords, leads, across, dir, gain, max_m, pop_m, weights }`; head / tail lamps in `FrameFront` / `FrameRear` bound to their end of the car | `lead/round12` (its "format 3") |

Readers should accept format 4 only (`validate-car-asset` and `tests/assets.rs` do).

```jsonc
{
  "id": "debug_coupe", "lattice_id": "debug_coupe", "format": 4, "tri_count": 16617,
  "tints": { "caliper": [200, 30, 30], "rim": [184, 150, 62] }, // optional: per-car material colours (sRGB)
  "livery": { "scheme": "...", "colors": [], "text": [] }, // optional: hint for render/livery.ts
  "atlas": { "y": [-2.35, 2.35], "z": [0.0, 1.62], "x_half": 1.04 }, // livery atlas frame (m)
  "wheel_camber_deg": [0, 0, 1.5, 1.5],            // optional: visual camber FL FR RL RR (+ = top in)
  "parts": [ ... ]                                 // one part per line
}
```

Part fields: `name` (unique lower_snake_case), `region` (BodyRegion, `null` for wheel-space
parts), `kind`, `material`, `wheel` (0–3 FL FR RL RR, wheel-space parts only; hub space with
the axle on +X, +X = the car's +X for every wheel), `rigid` (optional, default false: every
vertex bound to one node with weight 255), `positions_mm` (xyz ints, mm, chassis space), `uvs`
(u, v in 1/65535), `indices` (CCW from outside), `skin_nodes` (4 lattice node ids per vertex)
and `skin_weights` (4 per vertex in 1/255, summing to 255), `fold` (optional, hood / trunk lid /
front fenders / rear quarters only: the crumple binding below). Renderer:
`apps/testbed-web/src/render/panels.ts` (unknown materials fall back to paint).

| `kind` | Space | Renderer behaviour |
|---|---|---|
| `panel`, `body`, `glass`, `light`, `mirror`, `interior`, `detail`, `lightbar`, `decal_carrier` | chassis, skinned | re-skinned from the lattice; swing / hang / detach with their region |
| `engine`, `suspension` | chassis, rigid (one node) | moved by the mesh matrix, never re-skinned |
| `tire`, `rim`, `brake_disc` | wheel | spin group (spins, steers, follows compression) |
| `caliper`, `upright` | wheel | wheel pivot (steers, follows compression, does **not** spin) |

| `material` | Look |
|---|---|
| `paint` | body colour (never textured) |
| `livery` | body colour, or the livery texture when `PanelBody.setLivery(tex)` is set |
| `glass`, `trim`, `rubber`, `metal`, `interior`, `taillight` | as before (`taillight` base `#8a0a0a`, readable in daylight) |
| `headlight` | lens; left (x < 0) and right parts get separate materials (`PanelBody` takes `{ left, right }`) so police cars can wig-wag them |
| `chrome`, `carbon`, `engine` (dark metal), `grille` (near-black), `exhaust` (polished) | fixed |
| `caliper` | dark grey unless `tints.caliper` (`MechStyle::caliper_rgb`); tinted cam covers share it |
| `rim` | rims and plastic hubcaps (metal rims are written in this slot): metal unless `tints.rim` (`WheelStyle::rim_rgb`: gold, white, bronze …); discs and hardware stay `metal` |
| `spring` | coil springs of coilovers / painted springs: `tints.spring` (`MechStyle::spring_rgb`, else the caliper tint), else dark grey |
| `accent` | second paint colour for stripes / accent bars: `tints.accent` (`CarStyle::accent_rgb`), else dark red |
| `underglow` | emissive strips under the sills (`underglow_l` / `underglow_r`, only on cars with `CarStyle::underglow_rgb`): glow in `tints.underglow` while the headlights are on (`PanelBody.setUnderglow`) |
| `lightbar_red`, `lightbar_blue`, `lightbar_clear` | lens; left (x < 0) and right parts get independent materials (`PanelBody.lightbarMaterials.{left,right}.{red,blue,clear}`) for flashing |
| `emissive_amber` | amber lens with a faint glow |

Any material colour can be overridden per car with `tints` (`underglow` sets the glow colour).
`PanelBody.materialFor(slot)` returns a slot's shared material and `PanelBody.amberMaterials`
the amber one(s), for code that animates them (police lights).

### Crumple binding (`fold`, from round 12's format 3)

Hood, trunk-lid, front-fender and
rear-quarter parts carry `fold` (`car_assets::mesh::FoldBinding`), how the sheet crumples in the renderer —
visual only, the lattice, hull and repair bill never see it:
- `chords`: lattice node pairs from the fixed end (lid hinge row; the door-side row of a fender
  or quarter) to the leading end (latch row; fender nose; quarter tail), sorted along `across`
  (0 = x for lids, 2 = z for fenders and quarters);
- `leads`: per chord the three front-structure nodes nearest its leading node (hood: bumper,
  frame, fenders; trunk: rear bumper, frame, quarters; fenders: bumper, frame; quarters: rear bumper, rear frame). When they moved
  toward the fixed end further than the panel's own leading node, the panel's front is pulled
  back by the difference, so it never floats over a crushed nose. A hanging bumper's nodes are
  rotated back about its mount (the crushed pose it was released in); a detached one's skipped;
- `gain` (visual crumple gain: hood 1.8, trunk 1.4, fenders and quarters 1.5), `max_m` (lid
  0.28 m, fender / quarter 0.12 m), `pop_m` (lids 0.09 m), `dir` (buckle direction: lids up, fenders and quarters outward and up);
- `weights`: 3 per vertex in 1/255 — crease shape (lids: main ridge a little behind mid-lid, a
  valley, a second ridge toward the latch, diagonal, tented at the centre line; fenders and
  quarters: one ridge between the wheel and the car's end), fixed-edge lift (lids), leading-end follow (0 at the fixed row → 1 at
  the leading row; its square shapes the latch pop).

Per chord the effective shortening `gain × max(own chord loss, lead push)` (1.5 cm dead zone)
becomes a fold height `0.7 × fold_height` (an inextensible strip pushed into a tent), capped at
`max_m`; once the lid is unlatched or hangs, the hinge edge bows up (0.45 × fold) and the latch
edge pops up by `pop_m`. The renderer keeps the largest shortening / pull-back per chord (sheet
metal does not spring back) and clears it when every bound node is back at rest (repair).
`car_assets::skin::deform_detail` and `car_assets::crumple::render_part` are the Rust reference
of `render/panels.ts`.
### Head / tail lamps (from round 12's format 3)

`headlight_l/_r` and `taillight_l/_r` (`kind: light`, materials `headlight` / `taillight`) have
region `FrameFront` / `FrameRear` and are skinned to the nearest bumper, hood/trunk, same-side
fender/quarter and frame nodes (`car_assets::skin::light_regions`), so they crush with the end of
the car (and travel with a crushed bumper). Style kits re-shape these four parts in place. Every
other lamp a style adds (fog lamps, DRLs, reverse lamps, light bars, roof / side markers, a trunk
light bar) is a normal `light` / `lightbar` part skinned to its own region (bumper, fender,
trunk, roof …) like any other part. Renderer: `apps/testbed-web/src/render/panels.ts`.

### Livery atlas (UVs of `paint` / `livery` parts)

Paint parts use **U 0–0.5** of the texture; other parts keep a side projection in U 0.5–1
(u = 0.5 + 0.5·(y − tail)/length, v = height). Each paint face is assigned to one band by its
normal (top if n.z > 0.55, else front/rear if |n.y| dominates, else left/right by n.x); vertices
are split where bands meet. Car space is normalised by the `atlas` box: y ∈ `atlas.y`
(tail → nose), z ∈ `atlas.z` (ground → top), x ∈ ±`atlas.x_half`. Each band is inset by 0.004.

UV rectangles (v up; with the default `flipY`, canvas row 0 is v = 1):

| Band | u | v | Mapping inside the band |
|---|---|---|---|
| left side (x < 0) | 0 – 0.25 | 0 – 0.5 | **nose at u = 0 → tail at 0.25** (reads front → rear from outside), z up |
| right side | 0.25 – 0.5 | 0 – 0.5 | **tail at u = 0.25 → nose at 0.5** (reads rear → front from outside), z up |
| top (hood, roof, deck) | 0 – 0.5 | 0.5 – 0.8 | plan view with the nose to the right: tail at u = 0 → nose at 0.5; car left (−x) at the top (v = 0.8) |
| front face | 0 – 0.25 | 0.8 – 1.0 | seen from ahead: car right (+x) at u = 0, z up |
| rear face | 0.25 – 0.5 | 0.8 – 1.0 | seen from behind: car left (−x) at u = 0.25, z up |

So text drawn left-to-right with plain `fillText` at a left- or right-side anchor reads correctly
from outside on both sides (no mirroring). On a canvas of W × H covering the whole texture
(u 0..1; the right half is unused by paint), canvas rows 0 – 0.5H hold the top / front / rear
bands and rows 0.5H – H hold the sides. Helpers in `render/panels.ts`:
`liveryUv(atlas, side, a, b)`, `liveryPx(atlas, side, a, b, W, H)` (car metres → canvas pixel;
`a, b` = (y, z) for sides, (y, x) for top, (x, z) for front / rear) and `liveryBandPx(side, W, H)`.
Debug: open the testbed with `?livery=grid` (or call `debugLiveryTexture(atlas)`) to paint
labelled bands, a 0.5 m grid and `FRONT` arrows on every car.
