# Integration: art (machine S, prompt 11)

Branch `ws/art`. Owns `docs/art/`, `tools/assets/`, `apps/testbed-web/src/render/` materials,
palette and post.

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
