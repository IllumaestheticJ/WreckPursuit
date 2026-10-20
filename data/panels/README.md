# data/panels — panel specs and car meshes (workstream R, prompt 21)

Owned by workstream R (`ws/panels`). Every car in `data/cars` has:

| File | What | Made by |
|---|---|---|
| `<car>.json` | Panel spec (`wp_damage::PanelSpec`): hinged/bolted panels, hinge + latch nodes, swing limits, mass, inertia, damping, thresholds | `cargo run -p car-assets --bin gen-panels` (first pass from the lattice; hand edits welcome) |
| `meshes/<car>.json` | Procedural low-poly mesh (`car_assets::mesh::CarMesh`), one part per body region + glass, lights, mirrors, interior, wheels; skinned to the lattice | `cargo run -p car-assets --bin gen-car-mesh` (runs `bind-skin`) |

Both are checked by `cargo run -p car-assets --bin validate-car-asset` and by
`cargo test -p car-assets` (which also fails if `meshes/` is stale, like the lattices test).
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

## Mesh (`meshes/<car>.json`)

`{ id, lattice_id, format: 3, tri_count, parts: [...] }`, one part per line. Part fields:
`name`, `region` (BodyRegion or `null` for wheels), `kind` (`panel | glass | light | mirror |
body | interior | tire | rim`), `material` (`paint | glass | trim | headlight | taillight | rubber
| metal | interior`), `wheel` (0–3 FL FR RL RR, wheel parts only, hub space with the axle on +X),
`positions_mm` (xyz ints, mm, chassis space), `uvs` (u, v in 1/65535; U 0–0.5 reserved for
liveries), `indices` (CCW from outside), `skin_nodes` (4 lattice node ids per vertex) and
`skin_weights` (4 per vertex in 1/255, summing to 255).

Format 3 (replaces format 2's `fold_chords` / `fold_weights`): hood, trunk-lid and front-fender
parts carry `fold` (`car_assets::mesh::FoldBinding`), how the sheet crumples in the renderer —
visual only, the lattice, hull and repair bill never see it:
- `chords`: lattice node pairs from the fixed end (lid hinge row; fender rear row) to the
  leading end (latch row; fender nose), sorted along `across` (0 = x for lids, 2 = z for fenders);
- `leads`: per chord the three front-structure nodes nearest its leading node (hood: bumper,
  frame, fenders; trunk: rear bumper, frame, quarters; fenders: bumper, frame). When they moved
  toward the fixed end further than the panel's own leading node, the panel's front is pulled
  back by the difference, so it never floats over a crushed nose. A hanging bumper's nodes are
  rotated back about its mount (the crushed pose it was released in); a detached one's skipped;
- `gain` (visual crumple gain: hood 1.8, trunk 1.4, fenders 1.5), `max_m` (lid 0.28 m, fender
  0.12 m), `pop_m` (lids 0.09 m), `dir` (buckle direction: lids up, fenders outward and up);
- `weights`: 3 per vertex in 1/255 — crease shape (lids: main ridge a little behind mid-lid, a
  valley, a second ridge toward the latch, diagonal, tented at the centre line; fenders: one
  ridge ahead of the wheel), fixed-edge lift (lids), leading-end follow (0 at the fixed row → 1 at
  the leading row; its square shapes the latch pop).

Per chord the effective shortening `gain × max(own chord loss, lead push)` (1.5 cm dead zone)
becomes a fold height `0.7 × fold_height` (an inextensible strip pushed into a tent), capped at
`max_m`; once the lid is unlatched or hangs, the hinge edge bows up (0.45 × fold) and the latch
edge pops up by `pop_m`. The renderer keeps the largest shortening / pull-back per chord (sheet
metal does not spring back) and clears it when every bound node is back at rest (repair).
`car_assets::skin::deform_detail` and `car_assets::crumple::render_part` are the Rust reference
of `render/panels.ts`.
Light lenses (`kind: light`) have region `FrameFront` / `FrameRear` and are skinned to the nearest
bumper, hood/trunk, same-side fender/quarter and frame nodes. Renderer: `apps/testbed-web/src/render/panels.ts`.
