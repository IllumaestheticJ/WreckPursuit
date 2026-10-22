# Garage workshop integration notes

- **Workstream / prompt:** garage workshop (project thread "Garage and car modification")
- **Branch:** `claude/project-thread-thv5w5`
- **Crates / folders owned:** `crates/wp_economy/src/parts.rs`, `data/economy/parts.toml`,
  the workshop functions in `crates/wp_wasm/src/career.rs`; later stages add the workshop scene in
  `apps/testbed-web`
- **Contract version built against:** 0.3.0

A Street Legal Racing Redline style garage: the car on a lift in a 3D bay, every part clickable,
parts come off into an inventory and go back on, bought from a parts catalog. The reference game
is a model for layout and flow only; all art, assets and text are our own. Plan and stages:
[Garage workshop plan](https://claude.ai/artifact/2hjxjoRjmTMZKyazqkPzJ2).

| Stage | What | Status |
|---|---|---|
| 1 | Parts model (`wp_economy::parts`), slot table, wasm exports | PR #43 |
| 2 | 3D workshop scene in the testbed | PR #43 |
| 3 | Visible cosmetics on every car, vinyl library, more cosmetic parts | PR #43 |
| 4 | Work on it yourself (DIY repair, paint booth, car info) | planned |
| 5 | Tuning and polish | planned |

## Exposes

### `wp_economy::parts` (methods on `Economy`)
| Call | Notes |
|---|---|
| `econ.parts: PartsTable` | `data/economy/parts.toml`: 66 slots (`SlotDef { id, name, group, set, repair, puncture, lines, required, blocked_by, stock_price, optional }`), validated against the mod catalog (lines exist, one kind per line, `blocked_by` is a DAG) |
| `car_parts(&OwnedCar) -> CarParts` | stored state, or derived from `mods` (stock everywhere, optional slots empty) |
| `derive_mods(&mods, &CarParts)` | the mod list the parts amount to; a set line (four wheels) only when all four match |
| `slot_condition(&car, &slot)` | `1 - severity` of the slot's repair line, `1 - puncture` for a tyre |
| `workshop(&acct, car, mode) -> WorkshopView` | slots (fitted part, condition, fitted blockers, why it cannot come off), missing required parts, non-part upgrades, inventory with the slots each part fits now |
| `part_catalog(&acct, car, mode) -> Vec<CatalogEntry>` | stock replacement per kind + every grade of the kind's lines, price, count (4 for a set), reason if not buyable |
| `remove_part(&mut acct, car, slot) -> u32` | needs the `blocked_by` slots empty; repair lines and puncture go with the part; refused if another mod needs it |
| `fit_part(&mut acct, car, slot, part, mode) -> PartFit` | slot and its `blocked_by` empty, same kind, stock parts of the same model only; a perfect part returns `restore` items for `DamageSim::apply_repair` |
| `buy_part(&mut acct, car, kind, grade, mode) -> Vec<u32>` | into the inventory; an aftermarket set comes as four parts for the mod price |
| `sell_part(&mut acct, part) -> Money` | `mod_resale_frac × value × condition` |
| `missing_parts(&car)` | names of empty required slots |

Account state: `OwnedCar::parts: Option<CarParts>` (absent until the first workshop operation),
`PlayerAccount::inventory: Vec<LoosePart>`, `PlayerAccount::next_part_uid`. All `serde(default)`, so
older saves load unchanged (profile version stays 2).

The mod tree keeps working: `install_mod` / `uninstall_mod` update a stored parts state (the shop
fits and takes off the parts itself); `record_damage` drops repair lines of empty slots (the
removed part carries them).

### wasm (`wp_wasm::career`)
| Export | Notes |
|---|---|
| `career_workshop_json(uid)` | `WorkshopScreen` = `WorkshopView` fields + `catalog`, `cash`, `def_id` |
| `career_remove_part(uid, slot)`, `career_fit_part(uid, slot, part)` | return the workshop JSON; go through the same structure check as mods (a cage cannot go in while the body is dented); fitting a new part repairs that area of the stored lattice and re-records the damage |
| `career_buy_part(uid, kind, grade)`, `career_sell_part(uid, part)` | `grade = ""` = stock replacement |

| `career_workshop_session(uid, scenario_json)` | installs a paused sandbox session with the car as it is (mods, saved dents, empty slots) and no run, for the 3D workshop; refused during a run or while impounded |

`career_start_run` refuses a car with empty required slots ("the car is missing parts: ...").

### Testbed (`apps/testbed-web`, `apps/ui-kit`)
- `src/workshop/room.ts`: the garage bay, built in code (concrete floor, block walls, roller door,
  two-post lift, bench, tool chest, tyre rack, shelving, lights). The car never moves: raising the
  lift lowers the room.
- `src/workshop/slotmap.ts`: panel-mesh part (region / kind / name) → slot id in `parts.toml`.
- `src/workshop/mode.ts`: `WorkshopMode` (open / close, part picking and highlight, empty slots
  hidden on the car, camera presets overview / engine / under / wheels, rebuild after a change).
- `render/panels.ts`: `PanelBody.partDefs`, `partAt(ray)`, `hideParts`, `highlightParts`.
- `scene.ts`: `setWorkshop(on)` hides the flat ground, grid and origin axes.
- `settings.ts`: `DEBUG_LAYERS` moved here from `main.ts`; `layers.hold(ids)` keeps debug drawing
  off while the workshop is open, developer mode included.
- ui-kit `garage/Workshop.tsx`: overlay (top bar with cash / missing parts / Drive, view and lift
  tools, part card, catalog, part list, inventory strip); `CareerGarage` gets a Workshop button.
- `e2e/workshop.mjs`: headless browser run (pick, remove, blocker, missing parts, catalog buy,
  fit, drive). `SHOTS=dir` writes screenshots.

### Cosmetics (stage 3)
- `data/cosmetics/visuals.json`: every mod id → the pieces drawn on the car, or
  `{"none": reason}` (engine internals, ECU and similar). Piece vocabulary in
  `data/cosmetics/README.md`. 136 of 157 mods are visible; `wp_economy/tests/visuals.rs` checks
  the table against the catalog and that every `Cosmetic` mod is visible.
- 49 new mods: 14 vinyls, bull bars (tube / guard / ram, line `push_bar`), ducktail, drift wing,
  swan-neck wing, canards, xenon / amber / halo lamps, LED roof bar, fog lamps, underglow (4
  colours), mud flaps, 4 exhaust tips, whitewall / raised-letter / red-line sidewalls, 5 rims,
  limo tint, window net, satin and chrome paint, shatter-proof glass (line `glass`, a real
  durability mod on the windshield and rear glass).
- `CosmeticKind` gains `FogLights`, `Underglow`, `MudFlaps`, `ExhaustTip`, `Sidewall`,
  `WindowNet` (wp_economy only; not a contract type).
- `src/render/cosmetics/`: `CarLook` (look.ts) builds the pieces from the car's panel mesh
  (`CarFrame`: bumper, hood, roof, lamp, exhaust and wheel anchors), so one piece fits every
  model. Builders: body.ts (aero, lamps, bars, plates...), wheels.ts (rims, tyres, brakes,
  springs...), cabin.ts (seats, cage, engine bay, material tweaks), vinyls.ts (18 vinyl designs
  painted in car metres into the livery atlas). Bolt-ons ride with their region's lattice and
  vanish when the region detaches; pieces are clickable in the workshop (`slotAt`).
- `e2e/cosmetics.mjs`: every visible mod on four models, four themed builds, lamps, vinyl gallery.

## Port onto lead/round12 (two cosmetic renderers)
round12 already draws mods on cars: `wp_economy::visuals` (`PartKind`, mod `visuals` lists) →
`entity_visuals_json` → `render/mod_parts.ts` `fitBuild`, skinned to the lattice, on every modded
entity. This branch's look (`data/cosmetics/visuals.json` → `render/cosmetics/`) is kept next to it,
and each mod is drawn once:
- `PartKind::of_cosmetic` returns `None` for the new cosmetic kinds (fog lights, underglow, mud
  flaps, exhaust tips, sidewalls, window nets), and a cosmetic value with no `PartKind::styles`
  entry (turbofan rims, satin / chrome paint, the new vinyls and lamp colours) derives no part.
  Explicit `visuals` lists are still checked strictly when the catalog loads.
- `VehicleView.setLook` leaves out the mods `fitBuild` already drew (`FittedBuild.drawn` ×
  `BuildVisuals.parts[].mod_id`). The look adds the rest.
- Lamp lenses: round12's smoked covers win; otherwise the look's lamp style (xenon, amber, halo).
- `visuals.rs` `every_visible_slot_mod_draws_something` also counts mods with pieces in
  `visuals.json`.
- `PanelBody.ownSlot` uses round12's `ownCopy`; part centres come from `centreOf` (round12's
  `PartData` has no `centre`); `pivotParts` is round12's `hubParts`.
- Gaps: the workshop picker can't select parts that `fitBuild` drew (they carry no workshop slot).
  The mods the old renderer draws keep its look, not this branch's pieces.

## Needs
- Nothing open. Stage 3 reads anchors straight from `data/panels/meshes/*.json`.

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| 1 | `EconomyReason::PartBought { part_kind, grade }`, `PartSold { part_kind, grade }` (codes 65, 66) | Workshop purchases are not installs; today buying an aftermarket part posts `ModInstalled`, a stock part `RepairPaid { labour_multiplier: 0 }`, a sale `ModRemoved` (`stock_<kind>` for stock parts) | economy feed, ui-kit ledger | open |

## Changes outside my crates
- `crates/wp_net/src/server.rs`, `crates/wp_wasm/src/garage.rs`, `crates/wp_wasm/src/run.rs`,
  `crates/wp_economy/tests/properties.rs`: `parts: None` in `OwnedCar` literals.
- `crates/wp_economy/src/{data,ledger,error,garage,repair,lib}.rs`: the slot table in
  `DataSources` / `Economy`, the new account fields, `PartRefused` / `UnknownPart` errors, parts
  sync in the mod tree, empty-slot lines in `record_damage`.
- `crates/wp_wasm/src/career.rs`: `change_mods` returns the closure's value; workshop methods and
  exports; the missing-parts start check; `car_session` shared by `start_run` and `showroom`.
- `apps/testbed-web/src/{main,scene,settings,render/panels,wasm/sim}.ts`, `index.html`: workshop
  wiring (see Testbed above).
- Stage 3: `render/panels.ts` (`wheelDef`, `onStyled`, `regionShift`, `regionGone`, `ownSlot`,
  `partHit`, chained `whenReady`), `vehicle_view.ts` (`setLook`, lamp styles, wheel visibility
  respects hidden stock parts), `main.ts` (`applyCareerLook` puts the career car's look on the
  player view), `workshop/{mode,slotmap}.ts` (cosmetic piece picking and highlight).

## Perf numbers
Workshop operations run on a button press, not per frame.

## Known gaps
- Refitting a damaged part puts its repair lines back but not its dents: if a new part was fitted
  in between, the lattice is pristine there and the next damage report drops the lines.
- An optional or body slot left empty does not detach the panel when driving yet (stage 4).
- No workshop hours yet: removing and fitting is free and instant (stage 4).
- Bolt-ons move rigidly with their region's mean displacement; they do not bend with the dent.
- Stock parts a look hides (rims under aftermarket rims) stay hidden until the view rebuilds.
- Paint colour is still the car's stock colour; the paint booth is stage 4.
- AI and remote cars show no look yet; only the player's career car does.
- The workshop part picker only knows parts the car meshes draw; slots with no mesh (gearbox,
  ECU, fuel pump and similar) are reached through the Part list.
