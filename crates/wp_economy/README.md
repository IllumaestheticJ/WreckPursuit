# wp_economy

**Owned by workstream 08 (economy, progression, mods).** Branch `ws/economy`.

Money, reputation, repair billing, capture consequences, impound, race payouts, the mod system
and the garage. A punishing but fair economy: you pay for what you break, getting caught hurts,
racing clean pays, and the garage lets you trade speed for durability.

## Shape

Pure functions over a small persistent state. Everything is a method on `Economy` (immutable,
loaded from data) that takes `&mut PlayerAccount` and returns `(value, Vec<EconomyEvent>)`:

```rust
let econ = Economy::embedded();                    // or Economy::load(Path::new("data"))
let (mut acct, events) = econ.open_account(player);
let (car, ev) = econ.buy_car(&mut acct, &CarSpec::from(&vehicle_def), Condition::Used)?;
let (fee, ev) = econ.enter_race(&mut acct, &setup, car)?;
let (pay, ev) = econ.settle_race(&mut acct, &setup, car, &result)?;
econ.record_damage(&mut acct, &damage_report)?;    // end of session
let quote = econ.quote_repair(&acct, car, &RepairOption::RepairAll, &shop, mode)?;
let (order, ev) = econ.repair(&mut acct, car, &RepairOption::RepairAll, &shop, mode)?;
let (outcome, ev) = econ.handle_capture(&mut acct, &capture_ctx, &mut rng)?;
let preview = econ.preview(&vehicle_def, &lattice_def, &mods, Some(mode))?;
store.commit(&acct, &events)?;                     // AccountStore: in-memory or SQLite
```

**From the game loop**, use `EconomySession` instead of calling these one by one: feed it every
drained `SimEvent`, give it each car's `DamageReport` at the end of the run, and it settles
damage, towing, race payouts, captures (fine, bail, impound / seizure / destruction), escapes
and the repair bill in one step:

```rust
let mut session = EconomySession::new(SessionConfig::new(mode));
session.bind(vehicle_entity, acct.player, car);           // at spawn
for ev in sim.drain_events() { session.observe(&ev); }    // every tick
session.set_day_phase(phase);                             // when the clock changes phase
if session.is_captured(vehicle_entity) { /* end the run */ }
let result = session.finish(&econ, &mut acct, &[damage_report], &mut rng)?;  // run over
store.commit(&acct, &result.events)?;
// result: cash before/after, payouts, fines, bail, towing, repairs paid, rep/bounty deltas,
// per car: status (Garage / Impounded / Seized / Destroyed), repair quote + order, pound status
```

`finish` order: record each damage report (OR-ing in what the session saw: wanted during the
run, `TotalLoss`; an uncaptured total loss is towed and charged) → races → pursuit endings in
the order they happened → `RepairAll` quote for each damaged car still in the garage, paid under
`RepairPolicy::PayIfAffordable` (the default; `QuoteOnly` leaves it to the garage UI). It is
all-or-nothing and per account: other players' bound vehicles stay pending for their own
`finish`. Police events are decoded by `PoliceSignal::from_sim_event` (the only code that knows
today's opaque `wp_police` JSON; with typed v0.3.0 events it becomes a `match`).

**Impound**: a capture records the damage first, then impounds the car; `impound_status` gives
the fee today, its daily growth and the days left before auction; `retrieve_from_impound` pays
the fee and returns the car with exactly the damage it went in with, to be repaired as usual.

- **All-or-nothing.** Every operation runs on a copy of the account and only writes back on
  success. A refused operation (`EconomyError`) never changes anything.
- **Auditable.** Each change is an `EconomyEvent { seq, day, reason, car, cash/rep/bounty/xp
  deltas, cash_after }`. Sequence numbers have no gaps; stores reject commits that do not
  continue the log. `EconomyEvent::to_sim_event()` wraps it in the contract's opaque
  `SimEvent::EconomyEvent` for the client feed.
- **Server-authoritative and deterministic.** No wall-clock time (the in-game day advances via
  `end_of_day`), randomness only from a passed-in `wp_math::Rng` (Simulation seizures), maps
  are `BTreeMap`, the estimator is polynomial-only.
- **wasm-clean by default.** The SQLite store (`rusqlite`, bundled C) is behind the `native`
  feature, which nothing reachable from `wp_wasm` enables.

| Module | What |
|---|---|
| `ledger` | `PlayerAccount`, `OwnedCar`, `DamageSnapshot`, `EconomyEvent`/`EventReason`, transactions, `AccountStore`, `InMemoryStore` |
| `sqlite` (`native`) | `SqliteStore`: `accounts` + append-only `events` tables, one transaction per commit |
| `repair` | `price_table` (→ contract `PriceTable` for `wp_damage`), `DamageReport`, `RepairOption`, shops, insurance claims, scrap, `reference_estimate` |
| `capture` | fines, bail, rep loss, impound/seizure/destruction, escapes, pound fees + auction, daily tick, cop XP + rewards |
| `pursuit` | `PoliceSignal::from_sim_event` (decodes today's police events), `PursuitTracker` + `settle_pursuit`: pursuit endings → capture / escape settlement (no `wp_police` dependency) |
| `session` | `EconomySession`: the end-of-run glue for `wp_sim` (sim events + damage reports → `RunResult`) |
| `payouts` | purses, buy-ins, clean bonus, rep, bounty while wanted, debt rules, pink slips, informants |
| `mods` | `ModDef`/`ModEffect`, catalog validation, install/uninstall rules, `apply_mods` → modified `VehicleDef` + `LatticeDef` |
| `garage` | accounts, buy/sell, floor car, install/uninstall, `preview`, valuation, insurance |
| `estimate` | performance estimator (0–100, top speed, grip, braking) and the class index |
| `career` | scripted multi-night career used for tuning (the 20-night scenario) |

Data: `data/economy/{prices,stakes,consequences,market}.toml`, `data/mods/*.json` (16 packs,
106 mods). The damage mode tables in `data/modes/` are read (not owned) for `repair_cost_scale`.

## Economy design

### Money rules
- Cash is whole currency units (`i64`). **Voluntary spends must be affordable**: purchases,
  repairs, entry fees, pound retrieval, premiums, informants.
- **Only penalties can create debt**: fines, bail, towing. Negative cash is debt.
- **Debt blocks every voluntary spend and every race with a buy-in** (`stakes.debt
  .max_entry_fee = 0`): in debt you can only enter free street meets (`RaceType::Meet`), whose
  winnings pay the debt down. Pink slips are blocked too.
- **Economy floor:** a player with no usable car can always get the junkyard beater for
  `min(price, cash)`. It never creates debt.

### Repairs ("you pay for what you break")
- `wp_damage` prices damage with the `PriceTable` from `Economy::price_table(class, value)`:
  reference parts costs × class `parts_scale` × clamp(value / class reference value), per-item
  labour hours (`prices.toml [region_labour_hours]`, `[component_labour_hours]`; a cabin is 14 h,
  a bumper 1.5 h) and `repair_parts_fraction` (0.3). A line at severity ≥ 0.6 replaces the part
  (full price); below that it is repaired for `0.3 × severity / 0.6` of the price. Labour = item
  hours × max(severity, 0.1). `repair::reference_line` is the same formula (a test pins its
  constants to `wp_damage`'s).
- Billing re-prices every line: `(parts + hours × class labour rate × district × shop) × mode
  repair_cost_scale` (Simulation 1.0, Arcade 0.5).
- Options: `RepairAll`, `RepairSelected(items)`, `DriveDamaged` (free; the car keeps its lattice
  state and `HandlingEffects`, so a car you could not afford to align drives crooked) and `Scrap`
  (salvage = `base × (0.08 + 0.32 × health) + 0.3 × mods price`).
- Shops: crooked shops charge ×1.15 labour (×1.6 for a wanted car) and never report; legit shops
  charge normal rates but report cars at wanted level 2+ (`EventReason::ShopReported`).
- Towing (flat by class) is a penalty charged when the sim reports the car was towed.
- Insurance (`basic` 50 % / `full` 80 % after deductible, premium per day as a fraction of value)
  does not pay for damage sustained while wanted and is voided by a capture while wanted. An
  unaffordable premium lapses the policy (it never creates debt).

### Getting caught
- Fine = level base × (1 + 15 % per prior capture, max 6) + session violations + a capped
  lifetime-violations surcharge, × mode scale (Arcade 0.6, Simulation 1.25). Bail = 50 % of the
  bounty. Rep loss by level. Bounty cleared (police heat should reset).
- **Arcade:** always impound + fine (a wrecked car is impounded wrecked).
- **Simulation:** impound + fine, plus 30 % seizure at level 4–5; a total loss at capture means
  the car is destroyed.
- Pound: retrieval = base fee + per-day fee by class; after 10 days the car is auctioned (the
  owner gets 10 % of value minus fees).
- Escapes add bounty and rep by level (× day-phase bounty multiplier).
- Cop side: XP for captures/assists/clean minutes, losses for civilian hits, self-wrecks and
  overspending prompt 06's risk budget; cash reward = base by level + 25 % of the racer's
  bounty (assists 30 % of that). `cop_level(xp)` from thresholds.

### Payouts
`winner purse = max(min, per_km × km) × class × day phase × mode × wanted`; places 1–4 get
100/50/25/10 %; clean race (no contact, no violations beyond speeding) +25 %. Buy-in =
`entry_frac × base purse × phase entry`. Night pays ×1.35 (entry ×1.2), Simulation ×1.2, racing
with the cops on you up to ×1.45 plus bounty. Pink slips: both consent, Simulation only by
default, 500 rep minimum; the car moves with its mods and damage, never its insurance.

### Mods
UG2 parts tree + Wreckfest durability. A mod has a slot, a **line** (tiers of one upgrade
replace each other, the old part is refunded at 40 %), tier, price, `requires` (satisfied by the
same line at an equal or higher tier, so upgrading a prerequisite never strands a dependent),
`class_whitelist`, optional `modes` (nitrous is Arcade only) and effects:

- `Vehicle { param, op, value }` — 33 dotted parameter paths (`engine.torque_scale`,
  `engine.turbo_boost`, `drivetrain.lsd_rear`, `suspension.ride_height_m`, `wheels.tire_id`,
  `aero.downforce_rear`, ...). Resolution per parameter: `(last Set or base) × ΠMul + ΣAdd`.
  Produces a **modified copy** of `VehicleDef` (validated by the contract loader); inertia scales
  with mass.
- `Durability { target, scale, mass_kg, material? }` — targets a region (beam areas + detachable
  break impulses), a component mount (intrusion thresholds) or `CabinCage` (adds diagonal
  structural beams between cabin nodes). Produces a **modified copy** of `LatticeDef`. The mass
  goes on the vehicle and is spread over the target's lattice nodes. **Strengthening always adds
  mass** (enforced at load time); lightweight panels (`scale < 1`) remove mass and weaken.
- `Cosmetic { kind, value }` — no gameplay effect, except body-kit pieces produce
  `DetachableRequest`s (existing group if the lattice has one, otherwise a request for prompt
  03's `gen-lattice`).
- `Tool { kind, tier }` — radar detector, scanner, run-flat, reinforced cooling, fake plates,
  nitrous → `ToolLoadout`.

Every durability mod documents its trade-off in `tradeoff` (e.g. roll cage: +28 kg high in the
cabin, raises the CG; reinforced front bumper: +20 kg in the nose, more understeer).

**Class index** (`Economy::class_index`): `100 + 2.2·W/kg + 250·(lat g − 1) + 80·(brake g − 1)`,
clamped 100–999, brackets D <300 ≤ C <450 ≤ B <600 ≤ A <750 ≤ S <900 ≤ X. Debug sedan = 337 (C),
debug coupe = 559 (B). Monotone in power and in lower mass (property-tested).

### Tuning levers (and the intended feel)

| Lever | File | Feel target |
|---|---|---|
| `labour_hours_per_item`, `region_parts`, `component_parts`, `class.*.parts_scale` | prices.toml | bumper scrape < one buy-in; medium front hit ≈ one race win in Sim (half in Arcade); full wreck ≈ 2× value in Sim (≥ 60 % hard floor) |
| `data/modes/*.toml repair_cost_scale` | (workstream 03) | Arcade repairs half of Sim |
| `shop.*`, `district_labour` | prices.toml | crooked = convenience tax; legit = risk of a report |
| `race_types.*`, `day_phase.*`, `class_purse`, `mode_purse` | stakes.toml | Compact 9 km night sprint win ≈ 2 800; mid-skill nets ≈ 3 000 / night before repairs |
| `wanted_purse`, `bounty.*` | stakes.toml | racing hot pays, but captures cost more than it pays |
| `fine_by_level`, `mode.*.fine_scale`, `bail_bounty_frac` | consequences.toml | an L5 bust costs ≈ 2 nights in Arcade, ≈ 4 in Sim |
| `mode.Simulation.seizure_chance`, `total_loss_destroys_car` | consequences.toml | Sim can take your car |
| `impound.*` | consequences.toml | retrieve within a few days or lose it |
| `debt.max_entry_fee`, `race_types.Meet` | stakes.toml | debt is a slow grind out, not a game over |
| `starting_cash`, `floor_car`, `class_bands`, `condition`, resale fractions | market.toml | always a way back onto the street |

### Prices against the car roster

`tests/roster_prices.rs` checks the feel targets on every `data/cars` file (09's 10 street cars
and 3 police cars) and prints this table (`-- --nocapture`). Simulation, night 5 km Sprint:

| Car | Class | Value | PI | Full wreck / value | Medium front hit | / win purse | Scrape | Win purse | Buy-in | Pound fee |
|---|---|---|---|---|---|---|---|---|---|---|
| junkyard_beater | Beater | 3 000 | 214 | 5.08 | 882 | 0.95 | 35 | 932 | 83 | 150 |
| vireo_xs | Compact | 7 500 | 297 | 3.18 | 1 384 | 0.74 | 55 | 1 863 | 166 | 400 |
| kestrel_ts | Sport | 22 000 | 427 | 1.97 | 2 592 | 0.82 | 95 | 3 167 | 282 | 700 |
| shiro_turbo | Sport | 28 000 | 460 | 1.82 | 3 090 | 0.98 | 110 | 3 167 | 282 | 700 |
| lindwurm_4t | Sport | 34 000 | 465 | 1.73 | 3 587 | 1.13 | 125 | 3 167 | 282 | 700 |
| sperber_32 | Sport | 36 000 | 500 | 1.70 | 3 752 | 1.18 | 130 | 3 167 | 282 | 700 |
| hakone_gt | Sport | 48 000 | 494 | 1.60 | 4 746 | 1.50 | 159 | 3 167 | 282 | 700 |
| ironside_440 | Muscle | 38 000 | 330 | 1.59 | 3 723 | 1.25 | 126 | 2 981 | 265 | 650 |
| rampart_50 | Muscle | 42 000 | 569 | 1.56 | 4 045 | 1.36 | 136 | 2 981 | 265 | 650 |
| hauler_1500 | Truck | 32 000 | 321 | 1.66 | 3 256 | 1.59 | 112 | 2 049 | 182 | 500 |
| lucciola_350 | Exotic | 120 000 | 619 | 1.49 | 11 201 | 1.50 | 363 | 7 452 | 662 | 2 500 |

Asserted for every non-police car: full wreck ≥ 60 % of value; scrape < buy-in; medium front
hit 0.5–1.6× a win in Simulation (0.3–1.0× in Arcade, which halves repairs and pays 1/1.2 of
the purse); pound base fee 1–8 % of value; every price × condition inside its class band
(`roster_prices_sit_inside_their_class_bands`); the floor car file matches `market.toml`; a car
costing ≥ 25 % more than another street car is not slower (`pricier_cars_are_not_slower`,
known outliers `hakone_gt` and `ironside_440`, sent to 09).

Changes this round: class bands widened (Compact min 9 000 → 4 000, Sport 25 000 → 12 000,
Muscle 22 000 → 14 000, Exotic 90 000 → 60 000, Truck 15 000 → 12 000) because they were
clamping Vireo XS and Kestrel TS up and erasing their Used / Worn discount; Sport
`parts_scale` 1.9 → 1.75 and Truck 1.5 → 1.3 (Hakone GT and Hauler front hits were 1.6–1.8× a
win); Muscle `reference_value` 34 000 → 40 000 (the roster's Muscle cars are 38–42 k). The
debug sedan career below does not move (none of these touch a Compact at 18 000).

Career on each roster car (`twenty_nights_on_roster_cars`, 32 seeds, same scripted racer):

| Car | Arcade median vs start | Simulation median vs start |
|---|---|---|
| vireo_xs | +358 % | +209 % |
| kestrel_ts | +279 % | +71 % |
| shiro_turbo | +216 % | −10 % |
| lindwurm_4t | +170 % | −32 % |
| sperber_32 | +157 % | −34 % |
| ironside_440 | +136 % | −44 % |
| rampart_50 | +118 % | −51 % |
| hakone_gt | +103 % | −62 % |
| lucciola_350 | +106 % | −70 % |

Arcade is solvent on every car (asserted: median ≥ 1.2× start). Simulation climbs fast in a
cheap car and loses money in an expensive one. Part of that is intended (cheap starter cars get
you off the beater quickly; an expensive car in Simulation is a risk), but part is the career
model: its win rate does not depend on the car, so a faster car never wins more. Revisit when
race results come from real races.

### Tuning results (20-night scenario)

`tests/career.rs`: a mid-skill racer (wins ≈ 22 %, podium ≈ 80 %, contact in 45 % of races,
light/medium/heavy crash 35/15/5 %, police attention 35 % of nights, 30 % of pursuits end in
capture) races 3 night races per night for 20 nights in a Compact, with the *same* scripted
draws in both modes. Simulation applies 2× crash severity, can total the car on a heavy crash
(50 %), and its capture rules. Starting net worth 22 000. 64 seeds:

| Mode | p10 | p25 | median | p75 | mean | ended in debt | avg captures | avg cars lost / scrapped |
|---|---|---|---|---|---|---|---|---|
| Arcade | 30 847 | 43 211 | **54 610 (2.5×)** | 60 766 | 52 261 | 0 / 64 | 1.9 | 0.03 / 0.09 |
| Simulation | 2 701 | 8 953 | **18 747 (0.85×)** | 28 156 | 21 312 | 4 / 64 | 2.2 | 0.48 / 1.92 |

Averages: Arcade earns 72.5 k, pays 16.3 k buy-ins, 19.1 k repairs, 4.5 k penalties;
Simulation earns 68.1 k, pays 12.2 k buy-ins, 28.9 k repairs, 8.8 k penalties. (Re-measured on
contract 0.2.0 with `wp_damage`'s repair-vs-replace pricing and per-item labour hours; the
previous numbers were within 2–7 % of these.) The test asserts:
Arcade — no run in debt, median ≥ 1.6× start, p10 ≥ start; Simulation — median and mean within
0.7–1.3× start, p25 below start, median ≤ 0.6× the Arcade median.

Tuning changes made to get there: Arcade fine scale 1.0 → 0.6 (one L5 bust was ending Arcade
careers through the debt → pound auction spiral), street-meet purse 70 → 110 per km (so debt is
escapable), pound auction 7 → 10 days.

## Tests

```sh
cargo test -p wp_economy                    # everything except the SQLite store
cargo test -p wp_economy --features native  # + SQLite store tests
cargo test -p wp_economy --test career -- --nocapture   # print the tuning numbers
```

- `tests/properties.rs` (proptest): cash goes negative only through penalties and debt blocks
  buy-ins; event sequences have no gaps; a full Simulation wreck costs ≥ 60 % of value for any
  class/value/mods/shop; every durability mod adds mass and improves at most one performance
  stat (on four cars and random builds); installed mod sets always stay legal
  (whitelist/requirements/modes); class index monotone in power and weight.
- `tests/garage_repair.rs`, `tests/capture_payouts.rs`: behaviour of every API.
- `tests/data_tables.rs`: data completeness, embedded copy == `data/`, every mod applies.
- `tests/stores.rs`: in-memory + SQLite stores, sequence conflicts, `SimEvent` round trip.
- `tests/pursuit_wiring.rs`: a real `PoliceSystem` capture settled through the economy (both modes), escape / safehouse / called-off settlement, tracker event-order rules.
- `tests/session.rs`: `EconomySession` replaying event lists recorded from a real
  `PoliceSystem` (`tests/fixtures/*.json`; re-record with `-- --ignored record_fixtures`):
  capture → fine → impound → pound fee grows → release with the same damage → repair bill;
  total-loss capture by mode; escape → bounty → repair paid; unaffordable / quote-only repairs;
  uncaptured total loss towed; races; all-or-nothing and per-account `finish`; auction
  countdown; the police decoder against `wp_police`'s encoder (every level and outcome); a live
  police run settles like its recording.
- `tests/roster_prices.rs`: feel targets on every roster car (table above).
- `tests/career.rs`: the 20-night scenario + determinism + the roster sweep.

## Performance

Not on the per-frame path (called on race end, garage visits, captures), so the per-frame
budgets in `CLAUDE.md` do not apply — except `EconomySession::observe`, which sees every
drained event (it only matches the event and decodes police wanted / pursuit-ended payloads). Measured on the cloud session container, native release
build (`cargo test -p wp_economy --release --test perf -- --ignored --nocapture`):

| Operation | Time |
|---|---|
| `preview`, 7 performance mods (top-speed sweep + 0–100 integration dominate) | 167 µs |
| `preview`, welded cage (lattice copy + cage beams) | 248 µs |
| `enter_race` + `settle_race` (incl. account copy) | 0.4 µs |
| `handle_capture` | 0.2 µs |
| `quote_repair` | 0.1 µs |
| full 20-night career (60 races) | 168 µs |
| `EconomySession::observe` (recorded capture stream, per event; runs in the sim's event drain) | 0.03 µs |
| `EconomySession::finish` (capture + impound) | 1.3 µs |

Browser numbers not measured yet (expect ~2× native).

## Known gaps
- Lattice-state blobs and partial repairs need `DamageSim::{save_state, restore_state,
  apply_repair}` (contract v0.3.0). Until the sim calls `record_damage` with real estimates, the
  career scenario uses `reference_estimate` (same pricing as `wp_damage`, synthetic severities).
- The career's win rate does not depend on the car (see the roster sweep).
- No `wp_vehicle::metrics` yet, so `preview` uses the built-in estimator (comparative, not
  lap-time accurate).
- Body-kit pieces only become detachable where the lattice already has a detachable group for the
  region; creating new groups is a `gen-lattice` extension (prompt 03).
- Data files have no generated JSON Schemas (economy types live outside `wp_contracts`); they are
  validated by typed loaders with `deny_unknown_fields` and completeness checks.
- CI runs without `--features native`, so the SQLite tests only run locally until CI adds that
  line (requested).
