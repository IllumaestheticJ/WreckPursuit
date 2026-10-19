# PROMPT 08 — Economy, reputation, repair billing, impound, and the UG2/Wreckfest-style mod system (`wp_economy`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`. You own `wp_economy`, `data/mods/`, `data/economy/`. You may read `wp_contracts`, `wp_damage` (repair estimate shape), `wp_police` (heat/bounty, capture events), `wp_vehicle` (what a mod can change). Branch `ws/economy`.

## Goal
A punishing but fair economy: you pay for what you break, getting caught hurts, racing clean pays, and the garage lets you trade speed for durability. Everything data-driven and server-authoritative; pure functions with a small persistent state so prompt 07 can call it and tests can hit it directly.

## Work
**Accounts & persistence** (`ledger.rs`): `PlayerAccount { cash, rep, bounty, cop_rank, garage: Vec<OwnedCar>, heat_history, stats }` with an `AccountStore` trait (in-memory + SQLite via `rusqlite` behind `native` feature; server uses SQLite). Every mutation is an `EconomyEvent` with a reason (auditable; prompt 07 forwards them to the client feed).

**Repair billing**: consume `wp_damage::RepairEstimate` line items → bill; options at the garage: `RepairAll`, `RepairSelected(regions/components)`, `DriveDamaged` (car keeps its lattice state and `HandlingEffects` — this is important: a dented car that drives crooked because you couldn't afford alignment is the vibe), `Scrap` (salvage value = f(remaining health, base price, mods installed × 0.3)). Labour multiplier by district and by whether the car is wanted (crooked shops charge more, legit shops report you). Price table: `data/economy/prices.toml` (per `BodyRegion` panel cost by `VehicleClass`, per `Component` cost, labour, towing).

**Capture consequences**: fines scaled by `WantedLevel` and violation history; **impound** (car moves to the pound; retrieval fee grows daily; after N days → auction loss) with mode differences: Arcade: always impound + fine; Simulation: impound + fine + 30% chance of seizure at level 4–5, and `TotalLoss` means the car is gone. Bail/heat reset mechanics; rep loss. Cop side: player cops earn `cop_rank` XP for captures, assists, clean driving; lose XP for collisions with civilians and self-wrecks (prompt 06's risk budget feeds this).

**Payouts & stakes**: race purse = base(route length, class) × night multiplier × wanted multiplier × clean-race bonus (no contact, no violations beyond speeding); entry fees/buy-ins; pink-slip races (both consent; Simulation only by default); bounties paid to cops for captures; informant tips cost money and reduce heat. Tables in `data/economy/stakes.toml` keyed by `DayPhase` and race type.

**Mod system** (`mods.rs`, `data/mods/*.json`): UG2 + Wreckfest hybrid. A mod = `{ id, slot, tier, price, requires: [ids], class_whitelist, effects: Vec<ModEffect> }` where `ModEffect` is one of:
- `Vehicle(param_path, op, value)` — targets `VehicleDef` fields (engine torque scale/curve offset, turbo, gear set, final drive, LSD type, spring/damper/ARB, brakes, tire compound, weight delta, aero) — applied by producing a **modified `VehicleDef`** (never mutate the base).
- `Durability(region or component, scale)` — targets `LatticeDef` beam areas/material swaps (roll cage = adds Cabin structural beams; reinforced bumper = higher yield front rails; skid plate; radiator guard; stronger hubs) and `ComponentMount` thresholds — applied by producing a **modified `LatticeDef`**. Durability mods **add mass** and often **reduce performance** (document each trade-off).
- `Cosmetic(kind, value)` — body kit pieces, paint, vinyl slot, wheels, lights, window tint; zero gameplay effect except that body kit pieces *do* become detachable regions (coordinate with prompt 03's `gen-lattice` extension points).
- `Tool(kind, tier)` — radar detector, scanner, run-flat tires, reinforced cooling (heat-related), fake plates (reduces unseen-violation reporting), nitrous (Arcade only; brief torque scale with engine damage risk).
Slots: Engine, Forced Induction, Drivetrain, Suspension, Brakes, Tires, Weight, Aero, Body (front/sides/rear/hood/roof), Protection, Cooling, Electronics, Paint, Vinyls, Wheels, Interior. Write ~80 mods across classes with sensible prices, and a `class_index` ("performance index") function so lobbies can bracket cars.

**Garage API**: `install/uninstall`, `preview(car, mods) -> (VehicleDef, LatticeDef, class_index, mass, est_0_100, est_top_speed)` (call `wp_vehicle::metrics` if available; otherwise a simple estimator), `sell/buy car` from a market table (`data/economy/market.toml` with price bands by class and condition), beater tier always affordable (economy floor), insurance option (premium per day, covers X% of repair, voided when wanted at capture time).

## Tests
- Property tests: an account can never go negative except via fines (debt mechanic: debt caps race entry until paid); repair of a fully wrecked Simulation car costs ≥ 60% of value; durability mods always add mass and at most one performance stat improves; mod requirements/whitelists enforced; class index monotone with power/weight.
- Scenario: 20 simulated nights of a mid-skill racer (scripted outcomes) ends solvent in Arcade and marginal in Simulation — tune tables until true and record the numbers.

## Deliver
`wp_economy`, data, tests, README (economy design write-up with the tuning levers and the intended "feel" numbers), `docs/integration/economy.md` (exposes: ledger API, garage API, payouts, capture handling; needs: repair estimates, capture/escape events, race results; contract requests: `EconomyEvent` payload, `RepairEstimate` promotion).
