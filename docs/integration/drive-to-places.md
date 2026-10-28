# Drive-to places and home garages (career)

Owner request, 2026-10-27: "make it so that all dealers/garages/workshops have to be driven to,
you can only set the waypoint on the map. You have your own 3 personal garages you unlock as you
get 0, 2000, 5000 rep (to which then you can pick which garage to spawn out of, from the career
menu)". Same day: "spawn points can only be done from career mode".

## What changed

- **Car lots are on the map.** `wp_wasm::garage_visit::WORLD_LOTS` gives every lot of
  `data/economy/lots.toml` an anchor in the combined world (`ashpeak`); a career session snaps it
  onto the nearest lane (`snap_to_lane`) and puts a 15 m ring there (pink in 3D, `$` on the map).
  Stopping inside opens the place panel: "Walk the lot" ends the run into the lot visit.
- **Three home garages** (`career.toml [[home_garages]]`: Dockside Lock-up / Port Vesper at
  Unknown = 0 rep, Spire Basement Garage / Ashpeak City at Known = 2 000, Harnell Barn / Harnell
  Crossing at Respected = 5 000). A garage opens for good once the account's *peak* tier reaches
  it (`Economy::home_unlocked`), so a bad night never locks one again. Positions:
  `garage_visit::HOME_GARAGES`. White rings / `H` dots.
- **Runs start in the chosen garage.** `Career::start_run` moves the player of a night run or
  free drive in the world (not races, not touge) to the chosen garage's anchor
  (`profile.home`, `career_set_home(id)`, default = the highest unlocked); `Session::new` spawns
  it on that garage's lane instead of a race grid, and the garage's place panel opens at once
  (the player starts parked in their garage). The main menu's "Spawn in" no longer affects
  career runs (`careerMap()` uses the garage's city).
- **The career screen only sets waypoints.** With a host that reports `CareerApi.place()`
  (`apps/ui-kit/src/garage/access.ts`):
  - repair shop: workshop, mods, tune, tyres, repairs at that shop (the shop picker is locked);
  - own garage: workshop, mods, tune, tyres (no mechanic);
  - safehouse: workshop;
  - car lot: sell to the dealer, walk that lot;
  - anywhere: insurance, pound, informants, bounty, picking the active car, the street beater.
  - A car that cannot be driven is worked on where it sits (since 2026-10-28, see
    `tow-service.md`): off a repair shop it gets "Tow to …" (the tow service), at the shop it was
    towed to its repairs open. With no drivable car, lots offer "Taxi" (the old visit).
  - Everything else shows "Waypoint" (Car lots tab, new Garages tab: own garages with
    "Spawn here", repair shops). The waypoint is a flag on the map plus a gold GPS line
    (`gps_route`, refreshed every 1.5 s; `apps/testbed-web/src/ui/waypoint.ts`); pulling into
    the place clears it.
- New wasm exports: `places_json()`, `gps_route(x, y)`, `career_set_home(id)`. `PlaceVisit`
  gained `id` and the kinds `Lot` / `Home`. `MapHud.setPlaceExtras` is a second overlay channel
  under `setExtras` (street races keep theirs).

## Known gaps

- Street races and touge battles still start from the career menu's pickers (straight onto the
  grid); only dealers, garages and workshops were asked to be drive-to.
- The server has no career, so none of this is there.
- Lots and homes have rings, not buildings.

## Tests

`cargo test -p wp_wasm --lib spawn_tests` (places on roads and apart, run starts in the chosen
garage, unlock and peak-tier rules); `npx vitest run src/garage` in `apps/ui-kit` (access rules);
the career block of `apps/testbed-web/e2e/run.mjs` (taxi to a lot, repairs at a shop).
