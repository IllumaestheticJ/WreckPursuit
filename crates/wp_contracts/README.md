# wp_contracts

**Owned by workstream 12 (integration lead) — created by prompt 01.**

Shared types, data schemas, events, traits and constants. **Read-only for every other workstream**: request changes under "Contract change requests" in `docs/integration/<you>.md`; the lead applies them, bumps `CONTRACT_VERSION` and logs them in `docs/CONTRACT_CHANGELOG.md`.

## Contents
| Module | What |
|---|---|
| `lib.rs` | `PHYSICS_DT` (1/240 s), `NET_TICK_HZ` (60), `VEHICLE_SUBSTEPS` (2), `MAX_VEHICLES` (64), `CONTRACT_VERSION` |
| `vehicle` | `VehicleDef` and parts (engine, drivetrain, wheels, suspension, brakes, steering) |
| `tire` | `TireDef` (simplified Magic Formula), `SurfaceType` |
| `material` | `MaterialDef`, `MaterialKind` |
| `lattice` | `LatticeDef`, `BodyRegion`, `Component`, detachables, component mounts, skin binding |
| `mode` | `ModeTable` (Arcade/Simulation damage tables) |
| `runtime` | `VehicleInput`, `InputSource`, `AssistProfile` (+ presets), `VehicleState`, `WheelState`, `FfbOutput`, `HandlingEffects`, `ContactImpulse`, `SignalState` |
| `events` | `SimEvent`, `ViolationKind` |
| `ids` | `EntityId`, `PlayerId`, `Role`, `DamageMode` |
| `time` | `TimeOfDay`, `DayPhase`, `night_factor()` |
| `repair` | `PriceTable`, `RepairEstimate` (needed by the `DamageSim` trait) |
| `net` | `ClientToServer`, `ServerToClient` skeletons |
| `traits` | `VehicleSim`, `DamageSim`, `WorldQuery` |
| `load` | `load_car` + per-type loaders, `parse_*` (for embedded data), `resolve_car`, validators, `LoadError` |
| `schema` | JSON Schema generation (`cargo run -p wp_contracts --bin gen-schemas` → `schemas/`) |

Serde notes: data structs use `deny_unknown_fields` (typos fail loudly); enums use serde's default
externally-tagged form (`"Fwd"`, `{"Awd": {"front_split": 0.4}}`) so the same types work with
compact binary codecs.

The test `tests/data_validation.rs` validates **every** file under `data/` against the generated
schemas and the typed loaders, and fails if `schemas/` is stale.

## Known gaps
- `PriceTable`/`RepairEstimate` are minimal; workstreams 03/08 are expected to request changes.
- `PoliceEvent`/`EconomyEvent`/`CaptainCommand`/`Baseline` payloads are opaque bytes until 06/07/08 define them.

## Tests

```sh
cargo test -p wp_contracts
```
