# Debris glow fix integration notes (lead helper)

- **Owner report:** "parts falling off are currently glowing".
- **Branch:** `lead/debris-glow`
- **Folders touched:** `apps/testbed-web` only (no Rust, no physics, no data).
- **Contract version built against:** unchanged.

## Root cause
Detached panels become visual debris in `PanelBody.spawnDebris` (`src/render/panels.ts`). It
copied each part's skinned positions and UVs into a new `BufferGeometry` but **no `normal`
attribute**, and drew it with the material the part had on the car.

1. **Missing normals (the glow, every car, every style).** Body paint and livery are smooth
   shaded since car-shapes phase 3b (their batches carry crease-split normals; the materials
   are not `flatShading`). A mesh without a normal attribute reads a zero normal in the shader;
   `normalize(0)` is NaN, so the lighting comes out NaN. Greybox draws that as solid black;
   low-poly dark's bloom pass spreads it into a white halo: the "glowing" part. Flat-shaded
   slots (trim, grille …) were fine because they take normals from screen-space derivatives.
2. **Live lamp materials on lenses (second, intermittent).** Lamp lenses on a detached panel
   (e.g. the hatch's fog lamps on its front bumper, `headlight` slot) kept the car's head-lamp
   material, so they glowed (emissive 2.4 in low-poly) whenever the car's headlights were on.
   A lens only went dark if the panel *hung* for at least one re-skin before detaching
   (`updateLight` runs after `spawnDebris` and skips hidden parts).

Not the cause: envmap, ghost/replay materials, workshop highlight, tone mapping flags.

## Fix
- `debrisGeometry(rest, pos, index, uv)`: debris geometry gets crease-split normals built from
  the part's rest pose (`render/crease.ts`, the same 38° crease the car's smooth batches use),
  so a fallen panel shades exactly like it did on the car.
- `debrisMaterial(kind, key, drawn, brokenLamp)`: body parts keep the material they were
  drawn with (taken from the part's batch, so re-materialed parts and a car's own paint copy
  are honoured); lamp lenses (`kind: "light"`) and every glow slot (head / tail lamps,
  indicators, light bar, amber, underglow) take the dark broken-lamp material.

## Exposes
- `__wp.debris()` → `debrisAudit()`: every live debris mesh with its car, part name / kind,
  position, whether it has usable normals, flat shading, emissive, broken-lamp material.

## Regression check
`npm run e2e:debris` (`e2e/debris_glow.mjs`): low-poly dark and greybox × day (13 h) and night
(23 h), headlights on both cars and the police light bar forced on. A head-on (kestrel_ts vs
sentinel_warden, `panels: true`) tears the front bumper off; a T-bone plus repeated side hits
tear a door off. Checks: every debris mesh lit with usable normals, nothing emissive, lamp
lenses on the broken-lamp material. 33 checks pass with the fix; with the old `spawnDebris`
16 fail (normals in every combination; fog-lamp glow in most).

Screenshots (`DEBRIS_SHOTS=dir`): `docs/media/debris-glow/before/*.jpg`,
`docs/media/debris-glow/after/*.jpg`, contact sheets `before_sheet.jpg` / `after_sheet.jpg`
(rows: greybox day, greybox night, low-poly day, low-poly night; columns: bumper, door).

## Changes outside my crates
None (testbed only: `src/render/panels.ts`, `src/main.ts` automation hook, e2e, README,
package.json script).

## Known gaps
- Debris normals are fixed at the moment of detachment (debris is rigid, so that is exact).
- The door case needs repeated hits (`sim.debugPlace`): a single T-bone only unlatches /
  hangs doors in Simulation mode. Physics untouched.
