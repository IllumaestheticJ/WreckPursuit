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
| 4 | Engine internals: parts inside the engine, engine stand, wear and DIY rebuild | PR #43 |
| 5 | Engine stand in the 3D workshop (strip and build part by part) | PR #43 |
| 6 | Work on it yourself (DIY repair, paint booth, car info; time cost from `wp_world::progression`, PR #45) | planned |
| 7 | Tuning and polish | planned |

Owner decision (2026-10-22): part depth is whole assemblies **plus engine internals**. The engine
comes out whole with its internals inside, or is stripped part by part in the car or on the stand.

## Exposes

### `wp_economy::parts` (methods on `Economy`)
| Call | Notes |
|---|---|
| `econ.parts: PartsTable` | `data/economy/parts.toml`: 79 slots (`SlotDef { id, name, group, set, repair, puncture, lines, required, blocked_by, stock_price, optional, parent, damage_share, torque_loss, wear_limit }`), validated against the mod catalog (lines exist, one kind per line, `blocked_by` is a DAG, parents are plain top-level slots not blocked by their own parts) |
| `parts.children(id)`, `is_assembly`, `assembly_of` | the 16 slots inside `engine` (manifolds, throttle body, valve cover, timing, cams, valves, head, gasket, oil pan and pump, pistons, rods, crank, clutch, flywheel) |
| `strip_part(acct, assembly_uid, slot)` / `build_part(acct, assembly_uid, slot, part_uid)` | engine stand on a loose engine; same removal order among its parts; a part goes on only over the required parts its removal frees (head over gasket); a stock part only into its own engine family |
| `wear_torque_factor(&OwnedCar)` | `Π(1 − torque_loss × wear)` over engine members, 1 while the engine's repair line is open |
| `car_preview(vehicle, lattice, &OwnedCar, mode)` (garage.rs) | `preview` of the car's mods with the wear factor on the torque curve; what a session should load |
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

### Engine internals (stage 4)
- **Assemblies.** `parent = "engine"` slots come out with the engine as `LoosePart::contents` and go
  back with it. With the engine in the car they come off one by one in removal order.
- **Engine families.** Stock internals carry `model` = the stock engine's car model, or the swap
  engine's mod id; they fit only that engine. A new engine (stock or swap) comes complete.
- **Wear.** `FittedPart::wear` (assembly members only). A damage report that raises the engine
  line's severity by `d` adds `d × damage_share` wear to each member (a repeated report adds
  nothing). Condition shown = `1 − wear`. A shop repair of the engine line sets all wear to 0.
- **DIY rebuild.** When every member is within its `wear_limit` (internals 0.05, block 0.5), the
  engine line is cleared and `Component::Engine` comes back in `PartFit::restore` (in the car), or
  the loose engine's damage is cleared (on the stand; fitting it then restores the lattice).
- **Power.** With no engine line open, worn members cost torque through `car_preview`
  (`wp_wasm::Career::car_files` uses it). While the line is open the damage sim's component
  health already takes the power.
- **18 new mods** (`data/mods/engine.json`): intake manifolds, big throttle body, headers, cam
  gears, street / race cams, stiff / titanium valvetrain, ported / race head, MLS head gasket,
  high-volume oil pump, forged / titanium rods, light / stroker crank. `engine_internals_1..3`
  keep their ids (saves) but are now piston sets on the `pistons` slot (hypereutectic, forged,
  high-compression), priced and tuned as one part of the engine instead of a whole rebuild.
- wasm: `career_strip_part(uid, assembly, slot)`, `career_build_part(uid, assembly, slot, part)`;
  `WorkshopScreen` gains `stands`, `torque_factor`, `SlotView.parent`, `InventoryView.stand_fits`.
- ui-kit: Engine stand modal (strip / fit / buy per internal), part list shows internals under the
  engine only while it is fitted, worn-engine badge in the top bar. `e2e/engine.mjs` pulls the
  engine, strips it to the crank on the stand and builds it back.

### 3D engine stand (stage 5)
- `src/workshop/engine.ts`: `buildEngine(StandView, grade)` draws the loose engine from its
  internals (inline four; V8 for the swap engine): open-deck block with liners, crank, rods,
  pistons, gasket, head, valves, cams, valve cover, timing cover, intake runners and plenum,
  throttle body, headers, oil pan and pump, flywheel, clutch. Each internal is a group with
  `userData.slot`; parts out of the engine are not drawn, so stripping shows what is under them.
  Aftermarket parts are anodised by tier; parts under 60 % condition look rusted.
  `buildStand()` is the stand on casters.
- `mode.ts`: the stand stands in the bay beside the car (`STAND_AT`), the first loose engine
  bolted to it and rebuilt when its parts change; picking includes the stand engine
  (`stand:<slot>` → `WorkshopState.standSelected`), `highlightStand`, camera preset `stand`,
  `pointOfStandPart(slot)` for automation.
- ui-kit: the stand parts panel sits at the side (the engine stays in view); a part clicked in 3D
  opens it at that row; hovering a row lights the part up. `e2e/engine.mjs` clicks the valve
  cover in 3D and checks that stripped parts disappear from the model.

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
- Paint colour is still the car's stock colour; the paint booth is stage 6.
- Only the first loose engine stands on the stand; others are worked on from the list.
- The engine in the car's bay is still the car's panel mesh (no internals drawn there).
- Buying a stock internal uses the family of the engine in the car; stock parts for a loose
  swap engine with no swap in the car must come out of another engine of that family.
- Internals wear only from crash damage, not from over-revving or mileage yet.
- Gearbox internals are not modelled (the gearbox is one part).
- AI and remote cars show no look yet; only the player's career car does.
- The workshop part picker only knows parts the car meshes draw; slots with no mesh (gearbox,
  ECU, fuel pump and similar) are reached through the Part list.
