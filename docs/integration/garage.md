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
| 1 | Parts model (`wp_economy::parts`), slot table, wasm exports | this branch |
| 2 | 3D workshop scene in the testbed | planned |
| 3 | Visible cosmetics on every car, vinyl library, more cosmetic parts | planned |
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

`career_start_run` refuses a car with empty required slots ("the car is missing parts: ...").

## Needs
- Stage 3 needs panel anchor data per car (bumper, roof, wheel arch positions) from
  `data/panels/meshes/*.json`; the meshes already carry `region` and `kind` per part.

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
  exports; the missing-parts start check.

## Perf numbers
Workshop operations run on a button press, not per frame.

## Known gaps
- Refitting a damaged part puts its repair lines back but not its dents: if a new part was fitted
  in between, the lattice is pristine there and the next damage report drops the lines.
- An optional or body slot left empty does not detach the panel when driving yet (stage 4).
- No workshop hours yet: removing and fitting is free and instant (stage 4).
- No visible parts yet: mods do not change the car's mesh (stage 3).
