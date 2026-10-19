# docs/design — game design documents

Owned by workstream 19 (prompt `prompts/19_world_design_docs.md`), branch `ws/design-docs`.
Docs only: no code. Other workstreams read these; changes go through 19 (or the lead).

| File | What | Main readers |
|---|---|---|
| [`GDD.md`](GDD.md) | Game design document: pillars, player fantasies, loops with timing, progression, economy intent, difficulty/assists, modes, session structure, onboarding, failure states, what is deliberately not in the game | everyone |
| [`CITY.md`](CITY.md) | Port Vesper, the 6 × 6 km city: districts, roads, river crossings, 12 safehouses, 6 garages, police stations + helipad, 15 named routes, day event circuits, hazards | 05 world, 06 police, 15 race, 11 art |
| [`city_schematic.svg`](city_schematic.svg) | Schematic map (generated from the same coordinates as the zones JSON) | everyone |
| `../../data/city/handmade_v0.zones.json` | Zones + POIs in the `wp_world` format (draft, no road graph yet) | 05 |
| [`POLICE_DOCTRINE.md`](POLICE_DOCTRINE.md) | In-fiction pursuit policy the AI tables encode, call signs, 150 radio line templates | 06 police, 13 audio, 16 UI |
| [`CARS_LORE.md`](CARS_LORE.md) | One paragraph of fiction per roster car, invented maker houses, body-kit/vinyl themes | 08 economy, 16 UI, 11 art |
| [`DEMO_SCRIPT.md`](DEMO_SCRIPT.md) | 3-minute single-player night-chase demo, moment by moment, plus a plain-language playtest checklist | owner, lead, playtesters |
| [`FEATURE_BACKLOG.md`](FEATURE_BACKLOG.md) | Proposed but unscheduled features, prioritised, plus rejected ideas | lead |

Conventions: numbers marked **(code)** are quoted from data files on `main`; everything else is
a design target. No licensed names (`CLAUDE.md` rule 9). Coordinates: metres, +X east, +Y north.
