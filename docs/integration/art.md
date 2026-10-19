# Integration: art (machine S, prompt 11)

Branch `ws/art`. Owns `docs/art/`, `tools/assets/`, `apps/testbed-web/src/render/` materials,
palette and post.

- **Workstream / prompt:** 11 — art direction (machine S)
- **Branch:** `ws/art` (based on `origin/lead/round12` this round)
- **Folders owned:** `docs/art/`, `docs/media/art/`, `tools/assets/`, `apps/testbed-web/src/render/{palette,style,post}.ts`
- **Contract version built against:** 0.3.0 (no contract or Rust changes)

## Status (round 12 assignment, 2026-10-19) — all six items done
1. **Shared palette** `apps/testbed-web/src/render/palette.ts`: streets, district looks
   (3 building tiers, ground, window colour + lit share, sodium/LED lamps), window warm/cool,
   lamp sodium/LED, signals, car paints, traffic paints, police red/blue, and time-of-day
   keyframes (sky, horizon/fog colour + density, lights, bloom threshold, exposure) for dawn /
   day / dusk / night. Base values from the picker's "low-poly dark" mode.
2. **The look on the testbed** (`render/style.ts`, default; `render/post.ts`):
   flat shading + palette on every greybox material, building shader (per-district façade
   tiers from `map_static_json` zones, procedural windows glowing at night, fading to their
   average with distance), 1837 instanced street lamps along the kerbs in Port Vesper (4 draw
   calls) with light pools, gradient sky dome, palette fog per hour, HDR target + bloom on
   lights only (threshold above any lit surface) + ACES. R's panel meshes keep their damage
   (paint/glass/trim recoloured, still flat-shaded). Weather still works on top: wet-road
   patch untouched (only colours change), weather fog wins when thicker, clouds grey the sky
   (`docs/media/art/rainy_night_chase.png`).
3. **Lights:** police bars glow 13/15 (≥ 3× any other light; checked in `e2e/run.mjs`), brake
   lights 3, headlights 2.4 + a ground pool ahead of the car at night, signals 2.5.
4. **Perf + toggle:** "style" selector in the testbed panel (greybox / low-poly dark), also
   `?style=greybox|lowpoly`, remembered per browser; `?msaa=0` drops the post target's MSAA.
   Numbers below.
5. **Screenshots** `docs/media/art/` (dawn/day/dusk/night chase, night greybox, police bar
   phases, downtown street + aerial, rainy night) — `node e2e/art_shots.mjs`.
   `docs/art/ART_BIBLE.md` (short).
6. No `wp_contracts` / Rust edits.

## Perf numbers
`node apps/testbed-web/e2e/art_perf.mjs` (night city chase, Port Vesper, headless Chromium on
SwiftShader = a software GPU; relative numbers only).

| Measurement (SwiftShader, 1280×720) | greybox | low-poly dark | low-poly dark `?msaa=0` |
|---|---|---|---|
| frame (driving, 8 s) | 769 ms | 881 ms (+15 %) | 591 ms (−23 %) |
| post stack alone (same frame, GPU-synced) | — | +45 ms over 558 ms scene (8 %) | same |
| post target 4× MSAA (SwiftShader multisamples in software) | canvas MSAA | +489 ms | — |

Bloom on/off makes no measurable difference; the post passes are ~8 % of a scene render and
the rest of the delta is MSAA, which SwiftShader does in software. **Laptop GPU estimate** at
1920×1080: bright pass 0.5 Mpx × 4 taps, downsamples + 4 blurs ≈ 0.4 Mpx × 5 taps, composite
2 Mpx × 3 taps ≈ 10 M fetches + 2.6 Mpx written: ~0.2–0.4 ms on an integrated GPU, **≤ 1 ms**.
The MSAA target replaces the canvas MSAA (the canvas is created without antialiasing when the
page starts in low-poly dark), so AA costs about what it did before, at twice the bytes
(half-float). If that ever shows on a phone, `?msaa=0` is the switch.

## Changes outside my folders (minimal wiring)
- `src/scene.ts`: `hemi`/`sun` public; `agentMaterial`, `agentColour`, `policeGlow`,
  `renderOverride` hooks; `onCityLoaded(root, cityId)`; constructor `{ antialias }` option.
- `src/vehicle_view.ts`: style re-apply on `artStyle.version`, lamp/light-bar glow, headlight pool.
- `src/signals_view.ts`: `setLampBoost(k)`.
- `src/render/panels.ts` (R's file): `PanelBody.applyStyle(lowpoly, paint)`.
- `src/main.ts`, `index.html`: style selector, `?style=`, `__wp.style` / `__wp.setStyle`.
- `e2e/run.mjs`: art-style check; `e2e/weather.mjs`: "weather off" compares fog against the
  style's own fog (`__wp.style.baseFogDensity`) instead of 0.
- New: `e2e/art_shots.mjs`, `e2e/art_perf.mjs`.

## Requests
- **Testbed / lead:** new views should take colours from `render/palette.ts` and, for lights,
  set `emissiveIntensity` from it when `artStyle.lowpoly` (see `vehicle_view.ts setMeta`).
- **World (05):** a per-building id (or vertex colour) in the greybox would let façade tiers
  vary per building instead of per façade plane; street-lamp positions in the city data would
  replace the kerb-based placement in `style.ts`.
- **Police (06) / lead:** the non-vehicle police units (`police_units()`, helicopters) are still
  boxes; a light-bar mesh for them is a small follow-up on my side once they are drawn properly.

## Known gaps
- Traffic agents that are not physical are still instanced boxes (palette paints, no lamps).
- Windows ignore real floor heights on hills (grid from absolute height).
- No scrape decals, skid marks or sparks yet (decal pool: prompt 11 later items).
- Light pools are flat discs at kerb height − 13 cm; on steep grades they can clip slightly.
- Switching to greybox after starting in low-poly dark draws greybox without canvas AA (reload
  with `?style=greybox` for the old look exactly).

## Lead feedback (round 12, 2026-10-19) — first assignment
The owner picked **stylised low-poly dark** (`docs/art/STYLE_DECISION.md`). Base this round on
`origin/lead/round12` (it holds the newest testbed: weather, panels, minimap, garage), not main.
1. `apps/testbed-web/src/render/palette.ts`: one shared palette (asphalt, kerb, sidewalk,
   building tiers per district, window emissive warm/cool, lamp sodium/LED, car paints, police
   red/blue, fog colour per time of day). Take values from the "low-poly dark" mode in
   `docs/art/style_picker.html`.
2. Apply the look to the city greybox and the car views on the published testbed: flat
   shading everywhere, palette colours, emissive windows/lamps/signals at night, distance fog,
   a light bloom pass on emissives only (own render target + full-screen shader; keep it
   cheap), ACES tonemap. Keep R's panel meshes and their damage (flat-shaded facets are the
   point). Keep weather (K/lead) working on top: wet road darkening and rain must still show.
3. Police light bars are the brightest thing in the frame at night; brake lights and
   headlights read clearly.
4. Performance: measure frame time before/after in the city chase (headless numbers are only
   relative) and keep the post pass ≤ 1 ms on a laptop GPU estimate. Add a debug toggle
   "style: greybox / low-poly dark" so the owner can compare.
5. Screenshots per time of day in `docs/media/art/`; update `docs/art/ART_BIBLE.md` (short).
6. Don't edit `wp_contracts` or Rust sim crates; requests go here.
