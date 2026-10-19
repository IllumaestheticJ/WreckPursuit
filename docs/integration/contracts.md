# Contracts & skeleton integration notes

- **Workstream / prompt:** 01 — architecture & contracts (then 12 — integration lead)
- **Branch:** `main`
- **Crates / folders owned:** `wp_contracts`, `wp_math`, `wp_sim`, `schemas/`, workspace config,
  `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`, `docs/STATUS.md`
- **Contract version:** 0.1.0

## Exposes
Everything in `crates/wp_contracts/README.md`; `wp_math` helpers; `wp_sim::Sim` (see its README).
Stub implementations in every other crate with final public entry points:
`wp_vehicle::Vehicle::new(&VehicleDef, &TireDb, pos, yaw)`,
`wp_damage::DamageModel::new(&LatticeDef, &materials)`, `wp_world::World::flat(time, ratio)`,
`wp_police::PoliceSystem::step(world, racers, events, dt, out)`,
`wp_economy::{PlayerAccount, AccountStore, InMemoryStore}`,
`wp_net::{Transport, Channel, encode_*/decode_*, native::serve_tick_echo}`.

## Needs
- 03: real `DamageSim`; calibrate `data/modes/*.toml`. Contacts already arrive (`wp_sim/src/contacts.rs`).
- 03 + 06 merged before the v0.2.0 contract bump (plan in `docs/CONTRACT_CHANGELOG.md`).
- 10: scenario `barriers` (→ `Sim::add_static_box`) and start speed (→ `Sim::place_vehicle`).

## Contract change requests
None (this is the contract owner).

## Changes outside my crates
Initial stubs for every crate/app (prompt 01 scope).

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `simcli` smoke_straight_line, 3360 ticks, 1 stub car (release) | cloud container | see `docs/STATUS.md` | — | — |

## Known gaps
See `docs/STATUS.md`.
