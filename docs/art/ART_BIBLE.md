# Art bible — stylised low-poly dark (short)

Owner decision 2026-10-19 (`STYLE_DECISION.md`): **stylised low-poly dark** is the Checkpoint 3
look. Reference page: `style_picker.html` ("Stylised low-poly dark" mode). This file is the short
rule book; the code is `apps/testbed-web/src/render/` (`palette.ts`, `style.ts`, `post.ts`).
Screenshots: `docs/media/art/`.

## The look in one paragraph
Untextured, flat-shaded facets in a muted palette with a slight blue bias. The city is dark
masses with lit windows; light comes from things that glow (windows, street lamps, signals,
car lamps, police bars) and a gentle bloom spreads only those. Soft distance fog fades the city
into the sky colour. Night stays readable: a blue moon fill keeps silhouettes, and **police light
bars are the brightest thing in the frame**. Dents read because every crumpled panel breaks into
visibly different facets.

## Rules for anything that draws
1. **Flat shading**, no textures (decals and UI excepted). Detail comes from geometry, facets and
   emissive shapes (the building windows are procedural in the shader, not a texture).
2. **Colours come from `render/palette.ts`.** Add a named entry there rather than a hex in your
   view. Paints are muted (saturation ≈ 0.85 after tonemap).
3. **Lights are emissive + bloom.** No real-time shadows by default; no extra real-time lights
   except the police siren point light. Glow strengths (linear, before tonemap):

   | Source | Strength | Note |
   |---|---|---|
   | Police light bar | 13 red / 15 blue | ≥ 3× anything else (checked in `e2e/run.mjs`) |
   | Street lamp head | 4 | sodium in Residential / Industrial / Port / Suburb, LED in Core / Commercial |
   | Brake light | 3 | running tail lights a quarter of that |
   | Signal lamp | 2.5 | |
   | Headlight lens | 2.4 | plus a ground pool ahead of the car at night |
   | Lit window | 0.8 | below the bloom threshold: glows, does not bloom |
4. **Bloom only on lights**: the bright pass threshold is ~1.1 at night and 2.6 by day, so lit
   surfaces never bloom. Do not push surface albedo/lighting above it.
5. **Damage readability beats prettiness**: dents must read at 30 m in the chase camera.
6. Keep the per-frame cost flat: instancing for anything repeated (lamps are 4 instanced draws
   for the whole city), no per-object materials in hot paths.

## Time of day (`palette.ts` `TIME_OF_DAY`, interpolated by hour)
| | Sky top → horizon | Fog (1/m) | Light | Bloom threshold |
|---|---|---|---|---|
| Night (21–4:30) | `#0e1120` → `#1d2030` | 0.0042 | blue moon fill, hemisphere ×3.2 | 1.1 |
| Dawn (6:30) | `#34405e` → `#8a7a80` | 0.0034 | low warm sun | 1.6 |
| Day (9–16) | `#5d7598` → `#9aa6b4` | 0.0016 | overcast-leaning, muted | 2.6 |
| Dusk (19) | `#2c2f4a` → `#8a5e52` | 0.0032 | low amber sun | 1.4 |

Weather sits on top: its fog wins when thicker, cloud pulls the sky toward the fog grey, wet
roads darken and gain sheen (`render/weather.ts`, unchanged).

## Districts (`DISTRICT_LOOKS`)
Each district has three façade tiers (picked per façade), a ground colour, a window colour
(warm homes, cool offices), a share of lit windows at night and a lamp kind. Core is the
busiest and coolest; Industrial and Port are mostly dark with sodium lamps.

## Damage readability checklist (30 m, chase camera)
- [x] Flat-shaded panels: a dent changes the facet normals, so it shows as a light/dark change
      even on a dark paint at night.
- [x] Paints stay mid-value (no near-black paints) so facets have contrast to work with.
- [x] Broken lamps go dark (`brokenLight`), so a smashed front reads even head-on.
- [ ] Scrape/scratch decals (needs the decal pool; later).

## Post stack (`post.ts`)
HDR scene target (half float, 4× MSAA) → half-res bright pass → 1/4 and 1/8 blurs → composite:
bloom, exposure, ACES, saturation 0.88, shadow lift (blue), vignette 0.22, grain 0.012. No
chromatic aberration (a neon-noir trick; can come back as a speed effect).

## Comparing with greybox
The testbed's **style** selector (or `?style=greybox`) switches back to the debug look at any
time; the choice is remembered per browser.
