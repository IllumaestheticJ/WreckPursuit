# Wide-tyre look integration notes

- **Workstream / prompt:** lead — wider tyres read at a glance (owner request 2026-10-31)
- **Branch:** `lead/wide-tyre-look`
- **Crates / folders owned:** `apps/testbed-web/src/render/cosmetics/stance.ts` (new), edits in
  `vehicle_view.ts`, `render/mod_parts.ts`, `render/cosmetics/{look,wheels,context}.ts`
- **Contract version built against:** unchanged (render only; no Rust, no data changes)

## What changed
The "Wider Tires" mod (`tires_wide`, +2 cm `wheels.width_m`, data/mods/tires.json) used to scale
the tyre mesh by the physics ratio (~+9 %), which nobody could see. It is now drawn as a
**stance**, per width grade (grade = one 2 cm step; the data has one grade today, the code reads
up to three so a later grade 2 / 3 reads as a further step):

| | Grade 1 (`tires_wide`) | Grade 2 | Grade 3 |
|---|---|---|---|
| Tread width (× stock tyre mesh) | 1.20 | 1.36 | 1.50 |
| Wheel pushed out | inner face kept + up to 4 cm (fills the arch) | + up to 5 cm | + up to 6 cm |
| Deep dish (rim set back, chrome lip + barrel at the bead) | 1.6 cm | 2.2 cm | 2.8 cm |
| Body-colour arch lip / flare | 1.8–4.5 cm | 2.4–5.5 cm | 3.0–6.5 cm |

The lip follows each arch's real edge (traced from the rest-pose mesh) and rides its fender's
lattice (goes with a detached fender, hides with a fender removed in the workshop). It is small
on roomy arches and grows into a bolt-on flare on tight ones, sized so the tread stays at least
8 mm (14 mm on steered wheels) inside its outer edge. Measured over the whole roster (39 cars,
e2e `wide_tyres.mjs`): no tyre leaves its arch at any grade, the inner face never moves in by
more than 3 cm at grade 1, the tread is +20 % on every car.

- **Physics untouched**: the sim's wheels (width, positions) are exactly what the mod already
  gave; the e2e checks it. No `DamageMode` branching.
- **Same on every car with the mod**: from the build's wheel widths (`entity_visuals_json`:
  player, AI opponents, lot cars with mods) or from a cosmetic look's mod list (career car,
  workshop preview, touge opponents). The look's list wins while a look is set, so previewing
  the part in the garage shows it and dropping the preview takes it off again.
- **Both art styles**: the lip uses the body paint material, the bead ring the mesh's chrome
  slot; PanelBody restyles both.

## Screenshots
`docs/media/wide-tyres/` (written by `node apps/testbed-web/e2e/wide_tyres.mjs`):
`chase_<car>_<style>_{stock,grade1}.png` (rear three-quarter chase view; kestrel_ts,
junkyard_beater, hauler_1500 greybox, kestrel_ts low-poly dark), `garage_<car>_{overview,wheels}_
{stock,grade1}.png` (workshop, part previewed), `lock_<car>_{stock,grade1}.png` (front wheels at
full lock and full bump on the two tightest arches).

## Known gaps
- The far-LOD instanced fleet (render/fleet.ts) still draws the physics width (no stance at
  distance).
- `nyx_proto` already runs its stock tyres outside the arch edge; its wider tread stands out the
  same way (counted as flush).
- Drag radials together with wider tyres multiply (drag scale × stance scale).
