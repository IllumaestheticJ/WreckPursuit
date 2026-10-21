# Contracts & skeleton integration notes

- **Workstream / prompt:** 01 — architecture & contracts (then 12 — integration lead)
- **Branch:** `main`
- **Crates / folders owned:** `wp_contracts`, `wp_math`, `wp_sim`, `schemas/`, workspace config,
  `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`, `docs/STATUS.md`
- **Contract version:** 0.3.0

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

## v0.3.0 applied (2026-10-19, branch `lead/contract-v0.3.0`)

Full list and per-workstream migration: `docs/CONTRACT_CHANGELOG.md` → "v0.3.0 — applied".
Built on `claude/laughing-newton-0xqr6q`; not merged to `main`.

**What's left (skipped on purpose, and why):**
- UI kit (`apps/ui-kit`, 16): its committed `src/generated/contracts.ts` and recorded fixtures
  still describe the v0.2.0 opaque `{kind, payload}` events; its reducers decode payload bytes.
  Regenerating (`npm run gen:types`, `gen:police`, `gen:fixtures`) and switching the decoders is
  the UI owner's migration (`apps/testbed-web` has no event mirrors, so no TS changed here).
- Economy ledger still keyed by `PlayerId` internally (converted 1:1 to `AccountId`);
  `ShopReported.district` stays `String` until `prices.toml` uses `DistrictId` keys (G).
- R5 (economy `RaceType` vs race `RaceKind`): needs G + L to agree on purses.
- A-2 / C9 (`Radio` priority / line id), UI-2 (`UnitOrder::Hold`), UI-1/UI-3/UI-9/UI-10 HUD and
  captain views on the wire, A-1/A-4/A-5/A-6 (audio contact materials, sustained contacts,
  applied throttle, siren pattern), UI-7 (`PartDetached.region`), W-6 (`Season`), MC-4
  (`ClassBracket`), TB-2/TB-5: not in any owner's "final v0.3.0" list; next batch.
- Meta MC-6 (`join_token`, reconnect token) is wire-only → F adds it to `wp_net::protocol`.
- Nothing emits `SimEvent::Race` / `WeatherChanged` from `wp_sim` yet (race and weather are not
  composed into the sim loop); helpers exist (`RaceSignal::to_sim_event`,
  `WeatherChange::to_sim_event`). The server / sim loop wiring is the next lead run.
- `wp_vehicle` does not read `weather_at`; `surfaces.toml` lacks a `PaintedLine` row; nothing
  reports `PaintedLine`; `semi_slick.json` does not set `treaded: false` (A, N, D, H).
- `wp_police` still uses its own `RoadGraph` (built from `RoadGraphView`); moving it onto
  `RoadNetworkQuery` is E's step. `RoadNetworkIndex::nearest_on_road` is brute force.
- `DamageSnapshot` lacks compressive plastic strain and per-beam energy (B's shape); restore
  zeroes them. Repair of one region can leave neighbouring beams stretched until the next
  contact wakes the lattice. B should own `wp_damage/src/snapshot.rs`.

**Gate results on this branch:** `cargo fmt --all --check`, `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo build --target wasm32-unknown-unknown -p wp_wasm`,
`cargo run -p lint-determinism` (0 errors) pass. `cargo test --workspace`: goldens match
(every tick hash unchanged; only the `contract=` header was re-blessed), scenario determinism
in-process and fresh-process pass. Pre-existing failures on the base branch
(`claude/laughing-newton-0xqr6q`), checked there:
- `wp_economy` `real_police_capture_settles_through_the_economy` — **fixed here**: `set_heat`
  emits no `WantedLevelChanged`, so the tracker saw `None`; it now uses `PursuitEnded.level`.
- `wp_race` `simcli_races::city_sprint_on_real_roads` — **still failing, not touched**: the test
  indexes scenario entities by the sim's vehicle list, which since run 6 also holds physical
  police / traffic (`index out of bounds: len 2, index 2`). L should filter samples to the
  scenario's entities.

## Lead prototype: dentable parked cars (2026-10-21)

`wp_sim/src/parked_link.rs`: with `Sim::set_parked_car(car)` a parked-car prop that a vehicle is
about to hit is swapped for a full `VehicleKind::Parked` vehicle (damage model included) and
turned back into a prop, keeping its damage, once it rests with nobody near. Details, limits and
cost in `crates/wp_sim/README.md`. No contract change.

Touched outside `wp_sim` (minimal, for the new `VehicleKind::Parked` arm and the opt-in flag):
- `apps/simcli`: scenario field `parked_damage` (default `false`, so goldens are unchanged;
  documented in `SCENARIOS.md`, owner 10 please confirm the name); `city_frame_cost
  --parked-damage`.
- `wp_wasm`: browser free drive sets `parked_damage: true`; `vehicle_meta` kind 4 = parked;
  parked cars send no engine audio. `apps/testbed-web`: `VEHICLE_KIND.parked`, drawn traffic grey.
- `wp_net`: `VehicleKind::Parked` replicates as `EntityKind::Traffic`.

Open: the testbed does not draw parked-car props, so in the browser a parked car is invisible
until it is swapped in (just before a hit). World/testbed should render `World::static_props()`.
