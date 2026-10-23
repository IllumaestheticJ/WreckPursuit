# Indicator lenses (playtest row 136: "turn signals look plopped on")

Close-ups of five body styles, front and rear three-quarter views from the car's left, with
the indicators lit (hazards: both sides) and unlit, from a fixed camera relative to the car
(test pad, noon, 640×360, PNG quantised to 256 colours).

Made by `apps/testbed-web/e2e/indicator_shots.mjs` (after `npm run wasm`):

```
cd apps/testbed-web
node e2e/indicator_shots.mjs ../../docs/media/indicators/after
CARS=stoat_16v,kestrel_ts node e2e/indicator_shots.mjs /tmp/shots   # any cars
```

## Cars

| car | style |
| --- | --- |
| `stoat_16v` | hatch |
| `arashi_t4` | tuner |
| `bulwark_400` | muscle |
| `satchel_20` | van (traffic) |
| `sentinel_cruiser` | police |

## Shots

`before/` and `after/` each hold `<car>_<front|rear>_<lit|unlit>.png` for the five cars above
(20 files per set):

- `<car>_front_lit.png`, `<car>_front_unlit.png`
- `<car>_rear_lit.png`, `<car>_rear_unlit.png`

**before/** — round 12 (69ad6a2d) meshes and testbed: every car draws generic
0.16 × 0.2 × 0.09 m amber boxes on its four body corners (render/fleet.ts `indicatorMarkers`),
on top of whatever lamps the mesh has. Meshes then had at most `indicator_front_*` /
`indicator_rear_*` parts (stoat both ends, bulwark front only, arashi / police none), drawn as static amber lenses that never
blinked; traffic vans had `side_marker_*` parts only.

**after/** — regenerated meshes (every car has `indicator_front_l/r`, `indicator_rear_l/r`,
optional `indicator_side_l/r` repeaters, data/panels mesh contract) and the testbed blinking
those lenses: VehicleView draws them through PanelBody (skinned, they dent and break with
their panel) with per-side blink materials (amber, or the saturated brake red for red-on-red
rear lamps in `taillight`, e.g. `bulwark_400` rear; emissive only while lit, a dark lens
otherwise); the instanced fleet builds its left / right indicator geometry from the same
parts of the mid-LOD mesh. Box markers remain only as the fallback on corners a mesh has no
lens for, and on the low-poly proxy.
