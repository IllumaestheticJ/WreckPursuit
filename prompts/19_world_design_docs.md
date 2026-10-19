# PROMPT 19 — City design, narrative, and the GDD (docs only, `docs/design/`)

Read `CLAUDE.md`, `docs/DESIGN_OVERVIEW.md`, every `prompts/*.md`. You own `docs/design/`. No code. Branch `ws/design-docs`.

## Goal
Turn the engineering plan into a Game Design Document and a city design the world/art/race teams can build against.

## Deliver
- `GDD.md`: pillars, player fantasies (racer / chase cop / captain), core loops with timing (a typical 20-minute night session; a daytime sanctioned event; an illegal day race), progression curves, economy intent numbers (reference `wp_economy`'s targets), difficulty/assist philosophy, modes, session structure, onboarding, failure states, what is deliberately *not* in the game (NFS-style invincibility, rubber-banding, pay-to-win).
- `CITY.md`: a fictional US-coastal city of ~6 × 6 km: districts mapped to `district_kind`s, landmarks, arterials and the highway loop, elevation, 12 safehouse sites, 6 garages (legit vs crooked), police stations and helipad, 15 named routes with type/length/difficulty/traffic character, day event circuits, and a hazard list (rail crossings, drawbridge, tunnels, port cranes). Deliver as text + an SVG schematic + a `data/city/handmade_v0.zones.json` draft in the `wp_world` zone format.
- `POLICE_DOCTRINE.md`: the in-fiction pursuit policy that the AI tables encode (so writers, UI and AI agree), radio call-sign scheme and 150 sample radio lines by situation for `wp_audio`.
- `CARS_LORE.md`: one paragraph of fiction per roster car and the body-kit/vinyl themes for `wp_economy` cosmetics.
- `FEATURE_BACKLOG.md`: everything proposed but not yet scheduled, prioritised with a one-line rationale each.
