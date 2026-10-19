# Design docs integration notes

- **Workstream / prompt:** 19 — city design, narrative, GDD (docs only)
- **Branch:** `ws/design-docs`
- **Crates / folders owned:** `docs/design/`; drafted `data/city/handmade_v0.zones.json` (05 owns
  `data/city/`; this file is a draft for 05 to adopt or regenerate)
- **Contract version built against:** 0.2.0 (docs reviewed against the v0.2.0 changelog)

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
- `docs/design/DEMO_SCRIPT.md` — 3-minute single-player demo script (night city → speeding →
  pursuit → escape/capture → repair bill) with what each moment proves and whether it works on
  `main` today. Requested by the lead. Its playtest checklist moved to `PLAYTEST.md`.
- `docs/design/DEMO_GREYBOX.md` — the 5-minute single-player greybox demo (round 6b task 1):
  setup (city-chase preset), controls, heat table, police per wanted level, escape/capture rules,
  the bill (repair + fine + impound), the "done" checklist, and a per-machine gap table with the
  critical path. All numbers quoted from `data/police`, `data/economy`, `data/modes` and the
  procedural road graph.
- `docs/design/PLAYTEST.md` — fill-in playtest form for the published testbed (round 6b task 2),
  with a table mapping each section to the owning machine.

## Needs
Nothing blocks this workstream. Design decisions waiting on others are in GDD §12.

## Contract change requests
None directly. Proposals for owners (details in `CITY.md` §9, GDD §12):

| # | Change | Why | Who is affected | Status |
|---|---|---|---|---|
| C1 | Build `handmade_v0.road.*` from `CITY.md` | design target city; **blocks measuring the §6 route lengths** (round 6b task 3) | 05 | proposed |
| C2 | `Zone.name` or `DistrictId` | districts named across crews, heat, labour, radio, UI | lead, 05, 06, 08, 15, 16 | `DistrictId` deferred to v0.3.0 by the lead; still wanted |
| C3 | Garage shop kind on POIs (legit / crooked) | economy already prices it | 05, 08 | proposed |
| C4 | Dynamic world entities: drawbridge, rail crossings + trains, ferry ramp, straddle carriers | city hazards | 05 | proposed |
| C5 | Covered-lane flag used by LOS queries (tunnels, trenches, parking structures) | helicopter/camera LOS | 05, 06 | partly: v0.2.0 added `WorldQuery::line_of_sight` (2D building footprints); a covered/tunnel flag is still needed so the helicopter loses LOS in Calloway Tunnel / Founders Cut / parking structures |
| C6 | Surface types `Cobblestone`, `SteelGrate` (or document `Metal` covers grates) | Old Harbor, Iron Bridge | lead, 02, 14 | proposed |
| C7 | Timed speed zones (school zone) as lane metadata | Mercer St | 05, 06 | proposed |
| C8 | Route ids R01–R15 and the four day circuits as 15's first event data | shared names | 15, 18 | proposed |
| C9 | Line id travels with `PoliceEvent::Radio` once typed | audio picks recorded lines, avoids repeats | 06, 13 | waits on typed `PoliceEvent` (deferred to v0.3.0) |

## v0.2.0 review (2026-10-19)
Docs only, so nothing broke. References updated: `WorldQuery::line_of_sight` exists (C5 narrowed to the covered-road flag); `DistrictId`, typed
`PoliceEvent` and `BodyRegion::Accessory` are deferred to v0.3.0 (C2, C9 and the CARS_LORE
open question now say so). `WantedLevel` moving to `wp_contracts` needs no doc change (the docs never named its crate).
No new contract requests. Lead feedback section was empty; no items assigned.

## Round 6b response (2026-10-19)
1. **Done:** `docs/design/DEMO_GREYBOX.md`. Critical path it found: E (+D) stop police ramming →
   lead + G feed `PursuitEnded` into `EconomySession` and export the end-of-run result to wasm →
   C + M one HUD, PURSUIT/SEARCHING/ESCAPED/CAPTURED banner, bill opens on capture with fine +
   impound → B `apply_repair` → I re-aims `golden/city_chase`.
2. **Done:** `docs/design/PLAYTEST.md`. `DEMO_SCRIPT.md`'s old checklist now points to it.
3. **Blocked:** `CITY.md` §6 route lengths need a Port Vesper road graph (C1, workstream 05);
   only `procedural_12x12` exists, which has none of those streets. Marked in `CITY.md` §6.
   Measured the procedural city instead for the demo: ring road 5.58 km/lap (72 km/h,
   2 lanes each way), central highway 1.49 km (88 km/h, 3 lanes each way), 36.6 km of 40 km/h
   local streets.

Notes for the lead (not edits I can make):
- `docs/STATUS.md` row J says audio is "not wired to the browser yet"; it is
  (`apps/testbed-web/src/audio/`, `main.ts`). Checkpoint 2 line could link `DEMO_GREYBOX.md` §4
  as its definition of done.
- Two heat readouts are mounted in the testbed (C's `police_hud.tsx` and M's `ui/overlay.ts`).
  `DEMO_GREYBOX.md` §4 asks for one; C and M should agree who keeps it.

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
- `DEMO_GREYBOX.md` §5 is a snapshot of 2026-10-19; it goes stale as machines land work.
  Refresh it each lead round (or the lead may take it over in `STATUS.md`).
- The heat-per-second and escape-time figures in `DEMO_GREYBOX.md` are computed by hand from
  the TOML formulas, not measured in a run.

## Lead feedback
(Written by the integration lead.)


## Lead feedback (round 6b, 2026-10-19) — head-start tasks

Contract **v0.3.0** is being applied in parallel by the lead (branch `lead/contract-v0.3.0`). Do not edit `wp_contracts`; build your work so it drops onto v0.3.0 (inherent methods / local types now, trait wiring after). Merge `origin/claude/laughing-newton-0xqr6q` (the lead branch, ahead of `main` until today's gate finishes) before starting. Goal of the project right now: the **single-player greybox demo**: drive in the city, raise heat, get chased by physical police, escape or get caught, and pay the repair bill.

1. Write `docs/design/DEMO_GREYBOX.md`: the 5-minute single-player greybox demo script. Cover start location, controls, how heat rises, what police do at each heat level, escape and capture rules, the repair bill screen, what "done" looks like, and a per-machine checklist of what is still missing (cross-check `docs/STATUS.md` and the integration docs).
2. A playtest feedback form (`docs/design/PLAYTEST.md`) the owner can fill after playing the published testbed.
3. Measure the `CITY.md` §6 route lengths once D's graph is on `main` (procedural city for now), or mark what blocks it.
