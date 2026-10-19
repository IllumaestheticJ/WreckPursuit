# Art style decision

**2026-10-19 — owner: "let's do low poly dark for now."**

Chosen: **Stylised low-poly dark** (option B of prompt 11). Runner-up: neon-noir. "For now" means
this is the Checkpoint 3 look; it can be revisited at the engine decision.

Reference: `docs/art/style_picker.html` (the page the owner chose from; published at
https://claude.ai/artifact/PWsTA4d1fCSD81ySbaEkwt). Its "Stylised low-poly dark" mode is the
target look: flat-shaded, untextured, a muted palette with a slight blue bias, emissive windows,
lamps and signs, soft distance fog, gentle bloom on lights only, and police light bars as the
brightest thing in the frame.

Why: dents read best (crumpled panels break into clearly visible facets at 30 m), it is the
cheapest style in the browser (no reflection pass, little post), and it needs the least art
from us (procedural cars and city work without textures).

Rules for every workstream that draws something:
- Flat shading (`flatShading: true` or per-face normals); no textures except decals and UI.
- Colours come from the shared palette in `apps/testbed-web/src/render/palette.ts` (machine S).
- Lights are emissive + bloom; no real-time shadows by default (one optional shadow caster
  for the player's car is allowed).
- Damage must stay readable at 30 m in chase cam; that beats prettiness.
