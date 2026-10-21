# Wave 2: style-police

The spec is `research_police.md`. Lettering uses only the fictional Port Vesper Police Department
(PVPD) and generic words (POLICE, HIGHWAY PATROL, PURSUIT, DIAL 911, unit numbers). The emblem is
an invented lighthouse inside a ring.

## What was built

### Geometry (`tools/assets/src/styles/police.rs`, `styles/police/kit.rs`)
- There are five body styles keyed by car id. Each has a `LiveryHint` (scheme id + colour tokens),
  a `WheelStyle` and plain black calipers. The vigil has blue calipers.

| Car | Version | Bar | Equipment | Wheels | Tris |
|---|---|---|---|---|---|
| `sentinel_cruiser` | "sedan" | L-2010 LED 1.22 m | push bar, driver spotlight, 3 pucks, trunk whip, amber traffic advisor, grille LEDs | Steel + small cap | 11295 |
| `sentinel_warden` | "old sedan", boxy 80s | twin rotators (2 domes per side, chrome reflectors, clear takedown box, chrome gutter clamps) | rubber bumper guards, **two** chrome A-pillar spotlights, 2 whips, chrome rub strip (one piece per panel, cut at the arches), box arches, nose-up stance | DogDish, tall sidewall | 12082 |
| `sentinel_wolfhound` | "patrol" | LED 1.35 m | push bar with LED cubes, black spotlight, 4 pucks, visor bar, rear-window bar (red/amber/blue), decklid lip | Steel black | 11597 |
| `sentinel_bastion` | "SUV" | LED 1.37 m, set forward | heavy 4-upright push bar with LED cubes, tow hooks, 4 pucks, rear-window bar | Steel 6-lug + centre cap | 11705 |
| `sentinel_vigil` | "sports patrol" | **slicktop** | PIT bumper, visor bar, grille LEDs, rear-window bar, pedestal wing with a red/blue LED strip, carbon splitter, hood vents | SplitSpoke black alloy, blue calipers | 10743 |

- Every car also has a partition (solid lower panel, polycarbonate pane, bars) and unit-number
  decal carriers. The carriers are `decal_unit_l/_r` (rear quarter, or front fender on the
  warden), `decal_roof` and `decal_trunk` (cruiser, warden, wolfhound), of kind `DecalCarrier`,
  2 mm off the loft.
- All lens parts are split per side (`_l` at x < 0, `_r` at x > 0) and use
  `LightbarRed/Blue/Clear`. Amber parts use `EmissiveAmber`. A test checks that no lens part
  crosses x = 0.
- Each item is a `Shape::Custom` fn that reads the per-car `Kit` table by `def.id` and places
  itself on the styled profile: roof top, windshield and rear-glass lines, door stations,
  arches.
- `cargo test -p car-assets --lib police` checks that all five cars have a style, livery, decals
  and partition, are within budget, have split lenses, and that the vigil has no roof bar
  while the warden has domes.

### Livery (`apps/testbed-web/src/render/livery.ts`)
- There is one `CanvasTexture` per model, 2048 × 1536. It covers atlas U 0–0.5 only
  (`repeat.x = 2`), cached, and gets mipmaps and anisotropy.
- Paint regions come from the mesh's own part boxes (door_fl/door_rl/roof/hood/trunk), so the
  tone splits sit on the real shut lines.
- Schemes:
  - `pvpd_bw`: black car; white doors, B/C pillars and roof; silver pinline; POLICE, PORT VESPER,
    a faint emblem and DIAL 911.
  - `pvpd_bw_classic`: square white door rectangle, extended lettering, solid emblem disc.
  - `pvpd_highway`: tan over umber split at the shoulder; gold pinstripe pair with a hockey-stick
    end; outlined gold HIGHWAY PATROL.
  - `pvpd_suv`: white; navy lower body, bumpers and hood; cyan band with green pinline that
    breaks into forward-raked chevrons.
  - `pvpd_pursuit`: an 18° diagonal black/white split with a blue accent, twin stripes on hood
    and deck, and italic lettering leaning forward on both sides.
- Text is drawn with plain `fillText` at car-space anchors, with glyph width corrected for the
  band's px/m. **Checked from both sides in screenshots:** it reads correctly on the left, the
  right, the top (aerial numbers read from behind, glyph tops toward the front) and the rear.
- Unit numbers come from a hash of the entity id (sim cars) or the `unit_id` (non-sim units):
  `C-12` (cruiser), `C-40..59` (warden), `L-L1` (wolfhound), `S-B3` (bastion), `C-H1`
  (vigil). The roof uses the same string without the hyphen. `applyUnitDecals` re-maps each
  decal carrier's UVs from its rest box and gives it a small cached per-string texture.
- The font is the system condensed stack (`Arial Narrow`, `Roboto Condensed`, …) plus an extra
  squeeze. No font download.

### Lights (`apps/testbed-web/src/render/police_units.ts`)
- The `PATTERNS` table holds every pattern in research §2: P-GUMBALL, P-ROTATOR, P-STROBE,
  P-LED-QUAD, P-LED-WIGWAG, P-LED-SIM and P-SLOW.
- Channels are L and R (lens sides, any colour), W (takedowns) and A (amber).
- Lens meshes strobe at full rate. The point lights follow `env` windows with 40 ms ramps, at
  2 Hz or less.
- Rotators use a rotating lobe with a mirror second lobe; L and R are 180° apart.
- `patternFor`: the warden uses P-ROTATOR. Every other car uses P-LED-QUAD, or P-LED-SIM when
  parked.
- `?flash=slow` switches every car to P-SLOW (reduced flashing).
- Each car's phase is `hash(id)`, so a convoy is out of sync and the phase stays stable when the
  unit list changes.
- `LightRig` drives `PanelBody.lightbarMaterials.{left,right}`, the amber materials, and two
  point lights (red left, blue right) placed at `lightbarAnchors` + 0.25 m.

### Non-sim units (`PoliceUnitsView`, same file)
- The model is chosen by `PU.model` → `sim.policeModels()`. The list is fetched again when an
  index is out of range (new session). The fallback model is `sentinel_cruiser`.
- Per model, static geometry is merged by material, with wheels placed from
  `data/cars/<id>.json` hub positions, plus the shared livery.
- Per unit there are lens and amber materials and decals.
- Helicopters are drawn as a simple box-and-boom placeholder.
- A fixed pool of 2 light pairs goes to the nearest pursuing units within 120 m. The pool size
  never changes, so shaders do not recompile.
- Tested with synthetic `police_units()` rows, one per model, day and night. In the city-chase
  preset every unit was already a sim vehicle, so `police_units()` was empty.

### Hooks
- `vehicle_view.ts`:
  - Any car whose mesh has a known police scheme gets the livery, decals and `LightRig` once the
    panels load, and `setMeta` drives the rig.
  - The old box bar only shows without panel meshes.
  - Police-kind vehicles and `sentinel_*` cars create two siren point lights at construction.
  - The lead's `lamp.visible = bodyOn && !panelsOn` change is kept.
- `scene.ts`:
  - `updatePoliceUnits` delegates to `PoliceUnitsView`.
  - `render()` records the camera position in sim space (used for light LOD).

## Screenshots
Saved to `/tmp/claude-0/-home-user/1061d6c1-01ae-51aa-aaa0-b18f6899236a/scratchpad/`:
- `police_<car>_{left,right,rear,top,night}.png`, for car = cruiser, warden, wolfhound, bastion,
  vigil.
- `shots/police_units_{day,night}{,_b}.png` (non-sim units view).

They come from a dev (unoptimised) wasm build of this branch.

## Requests for the lead
1. **`fulmine_v10` mesh is format 1** (roster output). Because of it,
   `cargo test -p car-assets --test assets` fails on this branch. It needs the global
   regeneration. All police meshes are up to date.
2. **Keep unit numbers when a unit becomes a sim vehicle:** please expose the `wp_police` unit id
   of sim police vehicles, for example in `vehicle_info()` or a 4th `vehicle_meta` field.
   Today, sim cars hash the entity id and non-sim units hash `unit_id`, so the number on a car
   changes when the unit is promoted.
3. **Wig-wag headlamps (P-HEADLAMP-WW) are not implemented.** `PanelBody` shares one
   `headlight` and one `taillight` material across both sides. It would need left/right lamp
   materials, like the light-bar split.
4. Amber parts are found by part name (`traffic_advisor_cells`, `*_amber`) because
   `PanelBody.mats` is private. A public `materialFor(slot)` or an `amberMaterials` list would be
   cleaner.
5. Decal materials do not follow the paint opacity fade in the lattice view (minor).
6. Reduced flashing is URL-only (`?flash=slow`). It is worth adding as a settings toggle.
7. Doctrine: the vigil call sign `<P>-H<n>` (Hotel) is new, as proposed in
   `research_police.md` §1.5. It needs a row in `POLICE_DOCTRINE.md` §8.1.
8. Wheels: the styles set `RimKind::Steel` / `DogDish` / `SplitSpoke`, `CapKind` and lug counts.
   They render as the foundation star rim until the mechanical agent's `wheels.rs` lands. Dog-dish
   caps should be chrome.
9. GPU memory is one 2048 × 1536 livery per police model, about 16 MB with mips, and at most
   5 models. Decal textures are 128 px tall and cached by string.
