# Design docs integration notes

- **Workstream / prompt:** 19 — city design, narrative, GDD (docs only)
- **Branch:** `ws/design-docs`
- **Crates / folders owned:** `docs/design/`; drafted `data/city/handmade_v0.zones.json` (05 owns
  `data/city/`; this file is a draft for 05 to adopt or regenerate)
- **Contract version built against:** 0.1.0

## Exposes
- `docs/design/GDD.md` — the game design document.
- `docs/design/CITY.md` + `city_schematic.svg` — Port Vesper city spec.
- `data/city/handmade_v0.zones.json` — 17 zones (`Zone { kind, density_base, polygon }`) and 40
  POIs (`Poi { kind, name, pos, heading_rad, lane: null }`): 12 `SafehouseCandidate`, 6 `Garage`,
  6 `PoliceStation`, 1 `Helipad`, 15 `RaceStartGrid`. Verified to deserialize into
  `wp_world::network::{Zone, Poi}` (scratch crate, not committed). Extra keys (`name` on zones,
  `district`, `shop`, `callsign_prefix`, `route` on POIs) are ignored by serde today; they are
  proposals (below). No code reads this file yet, so it cannot affect tests.
- `docs/design/POLICE_DOCTRINE.md` — pursuit policy mapped to `wp_police` mechanics, call signs,
  150 radio line templates (`L001`–`L150`) with tokens for `PoliceEvent::Radio`.
- `docs/design/CARS_LORE.md` — car fiction and cosmetic themes, proposed cosmetic mod ids.
- `docs/design/FEATURE_BACKLOG.md` — unscheduled features B01–B37, prioritised.

## Needs
Nothing blocks this workstream. Design decisions waiting on others are in GDD §12.

## Contract change requests
None directly. Proposals for owners (details in `CITY.md` §9, GDD §12):

| # | Change | Why | Who is affected | Status |
|---|---|---|---|---|
| C1 | Build `handmade_v0.road.*` from `CITY.md` | design target city | 05 | proposed |
| C2 | `Zone.name` or `DistrictId` (already deferred to 05's merge in the v0.2.0 plan) | districts named across crews, heat, labour, radio, UI | lead, 05, 06, 08, 15, 16 | proposed (supports existing 08 #6) |
| C3 | Garage shop kind on POIs (legit / crooked) | economy already prices it | 05, 08 | proposed |
| C4 | Dynamic world entities: drawbridge, rail crossings + trains, ferry ramp, straddle carriers | city hazards | 05 | proposed |
| C5 | Covered-lane flag used by LOS queries (tunnels, trenches, parking structures) | helicopter/camera LOS | 05, 06 | proposed |
| C6 | Surface types `Cobblestone`, `SteelGrate` (or document `Metal` covers grates) | Old Harbor, Iron Bridge | lead, 02, 14 | proposed |
| C7 | Timed speed zones (school zone) as lane metadata | Mercer St | 05, 06 | proposed |
| C8 | Route ids R01–R15 and the four day circuits as 15's first event data | shared names | 15, 18 | proposed |
| C9 | Line id travels with `PoliceEvent::Radio` once typed | audio picks recorded lines, avoids repeats | 06, 13 | proposed (`POLICE_DOCTRINE.md` §10) |

## Changes outside my crates
- `data/city/handmade_v0.zones.json` (new file in 05's folder): a draft, clearly labelled in its
  `source`/`note` fields. 05 may move, regenerate or delete it.

## Perf numbers
Not applicable (docs only).

## Known gaps
- No road graph for Port Vesper yet: POIs have `lane: null`; route lengths in `CITY.md` §6 are
  targets ± 10 %, to be measured once 05 builds the graph.
- Winding roads are straightened in the schematic (Hollins Drive ≈ 1.9×, Bluff Road ≈ 1.15×).
- Heat thresholds, escape times and the highway-roadblock reading in `POLICE_DOCTRINE.md` §3 are
  proposals for 06 (marked there).
- Junkyard beater layout (FWD) in `CARS_LORE.md` is a guess until 09 adds the car file.
- Invented maker-house names were checked against the naming rule by hand only.

## Lead feedback
(Written by the integration lead.)
