# data/panels — panel specs and car meshes (workstream R, prompt 21)

Owned by workstream R (`ws/panels`). Every car in `data/cars` has:

| File | What | Made by |
|---|---|---|
| `<car>.json` | Panel spec (`wp_damage::PanelSpec`): hinged/bolted panels, hinge + latch nodes, swing limits, mass, inertia, damping, thresholds | `cargo run -p car-assets --bin gen-panels` (first pass from the lattice; hand edits welcome) |
| `styles/<car>.toml` | Style sheet (`car_assets::style::CarStyle`): personality, palette, body shape between the lattice anchors, lamps, bolt-on parts, wheels | Hand-written (see `tools/assets/README.md`, "Making a car look like itself") |
| `meshes/<car>.json` | Procedural low-poly mesh (`car_assets::mesh::CarMesh`, format 2) generated from the style sheet: body regions, glass, lamps, bolt-on details, interior, wheels; skinned to the lattice | `cargo run -p car-assets --bin gen-car-mesh` (runs `bind-skin`) |
| `RENDER_LIST.md` | Every car's personality, visual brief and mesh contents | `gen-car-mesh` (generated) |

Semi-trailers in `data/trailers` (e.g. `box_trailer`) have a style sheet (`bed = "trailer"`), a
mesh and a render-list entry, but no panel spec (`tools/assets/src/trailer.rs`).

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

`{ id, lattice_id, format: 2, tri_count, palette, parts: [...] }`, one part per line.
`palette`: `{ base, accent, primer, alts: [...] }` as 0xRRGGBB integers (the renderer paints
police and the first player in `base`, others in an alternative). Part fields: `name`,
`region` (BodyRegion or `null` for wheels), `kind` (`panel | glass | light | mirror | body |
detail | interior | tire | rim`), `material` (`paint | accent | primer | glass | glass_dark | trim
| chrome | metal | headlight | taillight | indicator | lens | beacon_red | beacon_blue | plate |
rubber | interior`), `wheel` (0–3 FL FR RL RR, wheel parts only, hub space with the axle on +X),
`hub_offset_mm` (wheel parts on a visual extra axle: hub offset from the physics wheel's hub,
chassis axes; omitted otherwise), `positions_mm` (xyz ints, mm, chassis space), `uvs` (u, v in
1/65535; U 0–0.5 reserved for liveries), `indices` (CCW from outside), `skin_nodes` (4 lattice
node ids per vertex) and `skin_weights` (4 per vertex in 1/255, summing to 255). Renderer:
`apps/testbed-web/src/render/panels.ts` (merges parts with the same region + material into one
draw call).
