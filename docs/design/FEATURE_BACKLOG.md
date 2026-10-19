# Feature backlog (proposed, not scheduled)

**Owner:** workstream 19. Everything here has been proposed in `docs/DESIGN_OVERVIEW.md` §4, the
design docs in this folder, or a workstream's "known gaps", and is **not** covered by any prompt
01–20 yet. When the lead schedules an item, move it out of this file and into that workstream's
prompt or integration doc.

Priority:
- **P1** — needed for Checkpoint 3 (low-poly playable, first external test) or the core loop feels broken without it.
- **P2** — makes the game noticeably better; schedule after Checkpoint 3.
- **P3** — nice to have; revisit once the game is fun.

"Suggested owner" is the workstream that already owns the closest code; the lead decides.

---

## P1 — before the first external test

| # | Feature | Suggested owner | Rationale |
|---|---|---|---|
| B01 | Handmade city v0 road graph (`handmade_v0.road.*`) from `CITY.md` | 05 | The procedural grid has no landmarks, chokepoints or hills; testers need a place, not a test fixture. |
| B02 | District identity (`DistrictId` / zone names) end to end | lead, 05 | Crews, heat maps, district labour, radio lines and UI all name districts; today they are indices. |
| B03 | Legit/crooked flag on garage POIs | 05, 08 | The economy prices the difference but the map cannot say which shop is which. |
| B04 | Covered-road flag for LOS (tunnels, Founders Cut, parking structures); `line_of_sight` itself landed in v0.2.0 | 05, 06 | Breaking line of sight is the racer's win condition; tunnels are the city's best tools for it. |
| B05 | Disconnect-while-wanted = capture after a 60 s grace | 07, 18 | Without it, quitting is the best escape and pursuits stop meaning anything. |
| B06 | Junkyard beater car file (`junkyard_beater`) | 09 | The economy floor references a car that does not exist. |
| B07 | Garage test lot (drive, crash test, compare before/after mods) | 04, 16 | Mods and repairs need somewhere to be felt (design overview §4). |
| B08 | Onboarding flow from GDD §9 (cold open, junkyard, first meet, first heat) | 15, 16 | First-session retention; the systems exist but nothing strings them together. |
| B09 | Shipped telemetry overlay + input display | 04, 16 | Serious players expect it, and testers' bug reports need it. |
| B10 | Assist presets (Rookie / Street / Sim) in settings | 16, 02 | One-click entry for new players without merging assists into damage mode (GDD §6). |
| B11 | Anti-cheat baseline: input-plausibility checks on the server | 07 | Server authority covers physics, not inhuman inputs; needed before any public test. |

## P2 — after Checkpoint 3

| # | Feature | Suggested owner | Rationale |
|---|---|---|---|
| B12 | Dynamic world entities: drawbridge lifts, rail crossings + freight trains, ferry ramp | 05 | The city's signature hazards (`CITY.md` §8); each one creates stories and police tactics. |
| B13 | Port straddle carriers (kinematic 60 t obstacles) | 05 | Makes the container terminal a living maze instead of static boxes. |
| B14 | Photo mode on top of replay | 04 | Free marketing and the most-shared feature in racing games; replay already exists. |
| B15 | Helicopter / spectator camera for the captain and spectators | 04, 16 | The captain's gameplay is a map plus a camera (design overview §4). |
| B16 | Dynamic crash scenes (wreck persists N minutes, traffic slows, a unit attends) | 05, 06 | "The city remembers"; also creates natural roadblocks and incident heat. |
| B17 | Escape reward bonus for committed safehouses (SH02, SH03, SH06) | 06, 08 | Rewards risk over hiding in the nearest bolt-hole (GDD §5.4). |
| B18 | Department standing: captain budget reduced after civilian-injury shifts | 06, 08 | Gives the captain a reason to call off a dangerous chase (GDD §4.3). |
| B19 | Timed speed zones (school zones) with double heat weight | 05, 06 | Cheap, adds daytime texture to Mercer, teaches "where you speed matters". |
| B20 | Surface types `Cobblestone` and `SteelGrate` | 02, lead | Old Harbor streets and the Iron Bridge deck (`CITY.md` C6). |
| B21 | Elevation in the road graph and greybox (hills, bluffs, viaduct, tunnel) | 05 | Hollins Drive touge and the bluffs need real grades; today's city is flat. |
| B22 | Radio line ids on `PoliceEvent::Radio` + recorded voice packs | 06, 13 | Lets audio play recorded lines instead of TTS and avoid repeats (`POLICE_DOCTRINE.md` §10). |
| B23 | New cosmetic values and themes from `CARS_LORE.md` §4–5 | 08, 11 | Districts and crews get a visual identity; cosmetics are the only monetisable thing allowed. |
| B24 | Livery editor (UG2-style vinyl layers) | 11, 16 | Long-requested by the audience; meshes already reserve the UV band. |
| B25 | Informant tips UI (pay to reduce heat) | 16, 08 | Economy supports it; nobody can reach it yet. |
| B26 | Accessibility pass: colourblind HUD palettes, rumble options, UI scale, full remap | 16 | Already partly in the UI-kit prompt; this is the audit + fixes after real screens exist. |
| B27 | Crew-vs-crew pink-slip challenge flow (lobby, consent, stakes UI) | 15, 16 | The rules exist in 08/15; the social flow around them does not. |

## P3 — later

| # | Feature | Suggested owner | Rationale |
|---|---|---|---|
| B28 | Cop vehicle loadouts (push bars, lightbar variants, ram-rated bumpers) | 08, 09 | Gives police their own garage progression. |
| B29 | Port Vesper radio stations (music + talk + traffic reports driven by sim events) | 13 | Atmosphere; traffic reports could hint at roadblocks. |
| B30 | Seasonal calendar (winter: early dark, ice on the bluffs; summer: long days) | 14, 15 | Changes the day/night balance over weeks. |
| B31 | Second city or a city expansion (airport, island) | 05, 19 | Only once one city is truly finished. |
| B32 | Ghost car sharing for time attack | 18 | Cheap social feature on top of replays. |
| B33 | Wheel force-feedback in the native client | 02, 04 | Native only (browsers cannot drive FFB); needed for sim players later. |
| B34 | Spectator mode for events (watch crews race) | 07, 16 | Community events and streaming. |
| B35 | Racer "heat history" visible on the captain map (repeat offenders flagged) | 06, 16 | Lets the captain prioritise known runners. |
| B36 | Truck-trailer delivery variant (articulated) | 02, 15 | Big physics showcase; expensive to build. |
| B37 | Weather-specific events (rain-only meets, fog runs on Bluff Road) | 14, 15 | Uses the weather system for content. |

## Considered and rejected

Kept here so they are not proposed again without new arguments. See GDD §11.

| Idea | Why not |
|---|---|
| Catch-up / rubber-band AI | Breaks pillar 5 (fairness). |
| Real-money purchase of cars, parts, cash or repairs | Pay-to-win; breaks pillar 5. |
| Heat reset at the paint shop | Removes the cost of being wanted; heat decays with time only. |
| Takedown cinematics that pause physics | Breaks pillar 1; replays show the real thing. |
| Pedestrians | Tone and scope; the game is about dangerous driving, not hitting people. |
| Police helicopter that can disable cars | The helicopter is eyes, not a weapon (prompt 06). |
| Full soft-body cars | Too heavy for 24 cars + traffic and in the browser (design overview §1). |
