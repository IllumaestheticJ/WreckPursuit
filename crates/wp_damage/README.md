# wp_damage

**Owned by workstream 03 (damage model).**

Semi-soft-body damage. **Currently a stub**: `DamageModel` implements `wp_contracts::DamageSim`, keeps the lattice rest pose, returns `HandlingEffects::NEUTRAL` and an empty `RepairEstimate`. `ModeTable`, `PriceTable`, `RepairEstimate` are re-exported from `wp_contracts`.

`box_lattice` + the `gen-box-lattice` binary generate the 120-node debug lattices in `data/lattices/` (`just lattices`); a test fails if the checked-in files are stale. `lattice_to_json_lines` writes one node/beam per line. Prompt 03 grows this into `gen-lattice`.

## Known gaps
- No deformation, detachment, component damage or repair pricing yet.

## Perf
Stub: negligible. Budget: ≤ 0.25 ms/car/frame native (24 cars).

## Tests

```sh
cargo test -p wp_damage
```
