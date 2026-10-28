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
| `lots` | dealerships / used lots / junkyards: `LotTables`, daily `lot_stock`, `Listing` + `ListingWear`, `buy_listing`, tyres (`replace_tires`, `tire_effects`, `wear_tires`) |
| `garage` | accounts, buy/sell, floor car, install/uninstall, `preview`, valuation, insurance |
| `estimate` | performance estimator (0–100, top speed, grip, braking) and the class index |
| `career` | scripted multi-night career used for tuning (the 20-night scenario) |

Data: `data/economy/{prices,stakes,consequences,market,lots}.toml`, `data/mods/*.json` (16 packs,
157 mods). The damage mode tables in `data/modes/` are read (not owned) for `repair_cost_scale`.

## Free car tune

`OwnedCar::tune` holds the car's four-slider tune (`CarTune`, 1..=5, 3 = stock;
`docs/integration/tune.md`). `Economy::set_tune` is free and writes no ledger line; the car must
be in the garage (like mod work). The field is `#[serde(default)]` and skipped when stock, so
career saves keep version 2. Tests: `cargo test -p wp_economy --test tune`.

## Garage workshop (`parts`)

Parts on a car: `data/economy/parts.toml` lists the slots (front bumper, radiator, the four
wheels...), the mod lines each takes and the removal order. Taking a part off puts it in
`PlayerAccount::inventory`; fitting takes it back; the car's `mods` are rebuilt from its fitted
parts, so the mod tree, preview and the sim need no change. Condition comes from the damage
snapshot. The engine is an **assembly**: 16 internals (`parent = "engine"`) come out with it,
or one by one in order, or on the engine stand (`strip_part` / `build_part`) once it is out.
Engine damage wears the internals by their `damage_share`; replacing the worn ones rebuilds the
engine yourself (the shop rebuild resets all wear). `clear_way` takes off whatever is in the
way of a slot in one go; `mechanic_install` pays the mechanic to swap a part (inventory or
bought now) and put everything back, selling or keeping the old one. Details and wasm exports:
`docs/integration/garage.md`.

```rust
let (part, _) = econ.remove_part(&mut acct, car, "front_bumper")?;
let (new, ev) = econ.buy_part(&mut acct, car, "radiator", Some("cooling_radiator_1"), mode)?;
let (fit, _) = econ.fit_part(&mut acct, car, "radiator", new[0], mode)?; // fit.restore → lattice
let view = econ.workshop(&acct, car, mode)?;
let (job, _) = econ.mechanic_install(&mut acct, car, "radiator", &PartSource::Buy(None), true, mode)?;
```

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
- **Resale never beats what you paid** (round 12b): each `OwnedCar` records
  `purchase_price` (the dealer price in the bought condition, or what the floor car cost; mods
  excluded). The dealer pays `dealer_buy_frac (0.7) × purchase_price + mod_resale_frac (0.4) ×
  mods`, scaled down by outstanding damage; scrap salvage of the body is capped at the same
  `0.7 × purchase_price`. Cars from v1 saves (no price) use the cheapest price the player could
  have paid (`Economy::resale_basis`: cheapest condition, or the floor price for the floor
  model). Insurance pays `coverage < 1` of the bill above a deductible > 0, so it always pays
  less than the repair.

### Repairs ("you pay for what you break")
- `wp_damage` prices damage with the `PriceTable` from `Economy::price_table(class, value)`:
  reference parts costs × class `parts_scale` × clamp(value / class reference value), per-item
  labour hours (`prices.toml [region_labour_hours]`, `[component_labour_hours]`; a cabin is 14 h,
  a bumper 1.5 h) and `repair_parts_fraction` (0.3). A line at severity ≥ 0.6 replaces the part
  (full price); below that it is repaired for `0.3 × severity / 0.6` of the price. Labour = item
  hours × max(severity, 0.1). `repair::reference_line` is the same formula (a test pins its
  constants to `wp_damage`'s).
- Billing re-prices every line: `(parts + hours × class labour rate × district × shop) × mode
  repair_cost_scale` (Simulation 1.0, Arcade 0.5). Districts are the contract `DistrictId`s
  (Core ×1.25, Commercial ×1.1, Residential / Suburb ×1.0, Industrial ×0.85, Port ×0.8);
  `district_from_key` also reads the pre-v0.3.0 keys (`default`, `Docks`, `Hills`).
- Options: `RepairAll`, `RepairSelected(items)`, `DriveDamaged` (free; the car keeps its lattice
  state and `HandlingEffects`, so a car you could not afford to align drives crooked) and `Scrap`
  (salvage = `base × (0.08 + 0.32 × health) + 0.3 × mods price`).
- Shops: crooked shops charge ×1.15 labour (×1.6 for a wanted car) and never report; legit shops
  charge normal rates but report cars at wanted level 2+ (`EventReason::ShopReported`).
- Towing (flat by class) is a penalty charged when the sim reports the car was towed.
- Insurance (`basic` 50 % / `full` 80 % after deductible, premium per day as a fraction of value)
  pays regardless of pursuit status (damage taken while wanted is covered if you get away). Only
  getting caught matters: a capture voids the policy, and damage outstanding at the capture is
  never paid, even by a policy bought afterwards. An
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
- Escapes add bounty, rep **and cash** by level (× day-phase bounty multiplier;
  `stakes.toml [bounty.escape_cash]`: night L1 455, L2 1 040, L3 2 080). Cash for losing the
  cops is the career's fast, risky money until races run in the browser: an L2 night escape
  beats an Arcade L1 bust in a beater (≈ 740) but not an L2 bust in a Sport car (≈ 1 810).
- Cop side: XP for captures/assists/clean minutes, losses for civilian hits, self-wrecks and
  overspending prompt 06's risk budget; cash reward = base by level + 25 % of the racer's
  bounty (assists 30 % of that). `cop_level(xp)` from thresholds.

### Career between runs (`profile`, step 2)
- `CareerProfile` is the browser save: account, ledger tail (`career.ledger_keep` = 200 lines),
  carried heat, career counters and each car's lattice damage (opaque text from the damage
  system). JSON with `format: "wreckpursuit-career"` and `version`; `migrate` refuses newer
  saves and steps older ones up (v1 → v2 adds `purchase_price: null` to every car).
- Each finished run is one in-game night (`close_run` → `end_of_day`: premiums, pound storage,
  auctions); `next_night` lets a night pass without driving.
- Heat: a capture or a wreck clears it; a run that ends free carries `heat × 0.5`, capped at
  45 (never above L2), forgotten below 5 (`consequences.toml [career]`). A legit shop that
  reports a wanted car raises the carried heat to that level's threshold (browser career).
- A new career starts with 4 500: a worn beater (1 800) leaves room for three busted L1 nights
  in it (≈ 740 each); `tests/browser_career.rs` keeps that true.

### Car lots (`lots`, `data/economy/lots.toml`)
Where cars come from in the career: **dealerships** (tiers 1–3: budget, mainstream, prestige),
**used car lots** (a bargain lot and a certified one) and **junkyards** (two yards).
- `Economy::lot_stock(lot, day, roster)` is a pure function of (lot id, day, roster): a
  `wp_math::Rng` seeded from the lot id and the day fills `slots` spaces with chance
  `fill_chance` each, from the roster models whose class and base price the lot carries.
  Every lot restocks daily and never shows every model every day; dealerships show each model
  at most once. Each slot forks its own RNG, so one space's roll never shifts another's.
- Dealership cars are new (`market_price(New) × price_scale`, health 1, new tyres).
- Used cars roll a condition in the lot's band and get wear to match (`ListingWear`): dents on
  up to seven panels, tired components (never below 0.35: they run), worn tyres. Price =
  `market_price(Used) × price_scale × (0.55 + 0.5 × condition)`.
- Junkyard cars roll a lower band, can go below 0.35 on any component, lose bolt-on panels
  (`missing_part_chance` each), do not run with `non_running_chance` (failed engine), get bald
  or flat tyres, and are priced at `price_frac` of the base price (×0.75 if dead). Their
  mechanical state is `hidden` until bought. With `rare_chance` a space holds a **rare
  variant** (`[[rare_variants]]`: a roster model with mods installed as found).
- `buy_listing` pays the asking price (`CarPurchased`) and adds the car with the listing's mods
  and tyres; `purchase_price` = the asking price, so resale never beats what was paid. The
  damage system turns the wear into lattice damage (browser: `DamageModel::apply_preowned_wear`),
  after which repairs work as for crash damage.

### Tyres
`OwnedCar::tires` (`TireState`: tread 0..1 and flat, per wheel; old saves load with new tyres).
**A repair never fixes tyres** — `RepairAll` and `RepairSelected` only cover estimate lines,
and tyres are not on the estimate. `replace_tires(car, wheels)` fits new ones at
`[tires] price_per_tire` by class (not mode-scaled: tyres are not damage); it is posted as an
`Adjustment` ("tires replaced: N") until the contract has a reason for it (request #8).
`tire_effects` gives the sim a grip multiplier per wheel (`bald_grip` at 0 tread → 1.0 new) and
a deflation (1 = flat); `wear_tires` takes `wear_per_hour × hours driven` off and keeps
spike-strip flats.

### Career progression (`progression`, `data/economy/career.toml`)
Credits are cash; rep is `PlayerAccount::rep`. On top sits a tier ladder decided by *current*
rep: Unknown (0) → Street (500) → Known (2 000) → Respected (5 000) → Feared (10 000) → Legend
(20 000). Tiers gate features (city betting at Street, touge at Known, touge side bets at
2 500 rep inside Known via `[unlock_rep]`, boss races and pink slips at Respected). Dealer classes have no rep gate: every car is for
sale from the start, so only its price holds it back. The first time a tier is reached its bonus is paid once (`career.peak_tier`); a capture
can drop you a tier and close what it opened. Bets are capped per tier and at 50 % of cash, and
refused in debt. The scripted mid-skill racer makes ~250 rep a night: Street after ~2 nights,
Known ~8, Respected ~20, Feared ~40 (`tests/progression.rs::tier_pacing`). API and ledger notes:
`docs/integration/career-progression.md`.

### Payouts
`winner purse = max(min, per_km × km) × class × day phase × mode × wanted`; places 1–4 get
100/50/25/10 %; clean race (no contact, no violations beyond speeding) +25 %. Buy-in =
`entry_frac × base purse × phase entry`. Races scheduled by `wp_race` carry their contract
`RaceKind` + `Legality` (`Economy::race_setup`): `[race_kinds]` picks the race-type row (Outrun →
Touge, TimeAttack → Circuit, Checkpoint / Cannonball / Delivery → Sprint) and a purse / buy-in
multiplier that keeps `wp_race`'s relative purses (checkpoint 0.8, cannonball 1.2, outrun ≈ 0.75,
time attack ≈ 0.55, drag ≈ 0.4 of a sprint); `[legality]` pays sanctioned events ×0.8 (buy-in
×1.2) and illegal daytime races ×4.5 (≈ 2.5× a night street race after the day-phase purse).
Night pays ×1.35 (entry ×1.2), Simulation ×1.2, racing
with the cops on you up to ×1.45 plus bounty. Pink slips: both consent, Simulation only by
default, 500 rep minimum; the car moves with its mods and damage, never its insurance.

### Mods
UG2 parts tree + Wreckfest durability. A mod has a slot, a **line** (tiers of one upgrade
replace each other, the old part is refunded at 40 %), tier, price, `requires` (satisfied by the
same line at an equal or higher tier, so upgrading a prerequisite never strands a dependent),
`supersedes` (coilovers carry their own dampers: installing or workshop-fitting them takes the
adjustable dampers off at resale value, the dampers are refused while coilovers are on, and
`Economy::settle_superseded_mods` clears old saves with both at a full refund),
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
- `Cosmetic` mods (and every other mod that can be seen) are drawn by the testbed from
  `data/cosmetics/visuals.json`; `tests/visuals.rs` keeps that table in step with the catalog.
- `Tool { kind, tier }` — radar detector, scanner, run-flat, reinforced cooling, fake plates,
  nitrous → `ToolLoadout`. The career (`wp_wasm::career`) hands them on: run-flat and cooling
  to `wp_sim` (`Sim::set_run_flat`, `Sim::set_cooling_tier`), radar / scanner / plates to the
  session's `PlayerGadgets` (map, radio, `Economy::carry_heat_with_plates`). Nitrous is wired
  by the nitrous workstream. See `docs/integration/mod-effects.md`.

**What a build looks like** (`visuals`, `Economy::build_visuals`): every mod that changes
something you could see on a real car says so, and a build resolves to `BuildVisuals` — the parts
to draw (`PartKind` + style + the body regions it is mounted on, so the renderer binds it to that
region's lattice nodes: it dents, swings and detaches with that panel), per-wheel tire width /
radius / compound from the *modified* `VehicleDef`, and the ride-height change. Parts come from
`Cosmetic` effects and `wheels.tire_id` automatically, and from a mod's optional
`"visuals": [{"part": "RearWing", "style": "gt"}]` list for parts whose effects are numbers
(aero, brake kits, cages, bars, exhaust, intercooler, supercharger, carbon panels). Styles are
validated at load; a test fails if any mod in a visible slot (wheels, tires, aero, body, hood,
roof, paint, vinyls) draws nothing. The browser draws them in `apps/testbed-web/src/render/mod_parts.ts`.

Every durability mod documents its trade-off in `tradeoff` (e.g. roll cage: +28 kg high in the
cabin, raises the CG; reinforced front bumper: +20 kg in the nose, more understeer).

**Class index** (`Economy::class_index`): `100 + 2.2·W/kg + 250·(lat g − 1) + 80·(brake g − 1)`,
clamped 100–999, brackets D <300 ≤ C <450 ≤ B <600 ≤ A <750 ≤ S <900 ≤ X. Debug sedan = 337 (C),
debug coupe = 559 (B). Monotone in power and in lower mass (property-tested).

### Tuning levers (and the intended feel)

| Lever | File | Feel target |
|---|---|---|
| `labour_hours_per_item`, `region_parts`, `component_parts`, `class.*.parts_scale` | prices.toml | bumper scrape < one buy-in; medium front hit ≈ one race win in Sim (half in Arcade); full wreck ≈ 2× value in Sim (≥ 60 % hard floor) |
| `respray_base` | prices.toml | paint shop respray (× class `parts_scale`): a few hundred on a beater, thousands on an exotic |
| `heavy_damage_markup`, `total_loss_markup` | prices.toml | scrapes stay cheap; crushed parts cost up to 1.75×; rebuilding a write-off costs 1.4× more, so a wreck is usually a scrap |
| `data/modes/*.toml repair_cost_scale` | (workstream 03) | Arcade repairs half of Sim |
| `shop.*`, `district_labour` | prices.toml | crooked = convenience tax; legit = risk of a report |
| `race_types.*`, `race_kinds.*`, `legality.*`, `day_phase.*`, `class_purse`, `mode_purse` | stakes.toml | Compact 9 km night sprint win ≈ 2 800; mid-skill nets ≈ 3 000 / night before repairs |
| `wanted_purse`, `bounty.*` | stakes.toml | racing hot pays, but captures cost more than it pays |
| `fine_by_level`, `mode.*.fine_scale`, `bail_bounty_frac` | consequences.toml | an L5 bust costs ≈ 2 nights in Arcade, ≈ 4 in Sim |
| `mode.Simulation.seizure_chance`, `total_loss_destroys_car` | consequences.toml | Sim can take your car |
| `impound.*` | consequences.toml | retrieve within a few days or lose it |
| `debt.max_entry_fee`, `race_types.Meet` | stakes.toml | debt is a slow grind out, not a game over |
| `starting_cash`, `floor_car`, `class_bands`, `condition`, resale fractions | market.toml | always a way back onto the street; a new player affords a beater and a few runs |
| `bounty.escape_cash` | stakes.toml | an escape is fast money; a bust in a real car costs more than the escape pays |
| `career.heat_carry_*` | consequences.toml | the police remember last night, but a night off cools them |

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

2026-10-24 (player feedback: wrecks were too cheap to fix): every billed line is now
× `1 + heavy_damage_markup × severity²` and a total loss × `total_loss_markup` (prices.toml,
applied in `Economy::price_lines`; 0.75 and 1.4). A full Shiro Turbo wreck went 0.91 → 2.2× its value, a
medium front hit 0.59 → 0.75× a win (Arcade), a bumper scrape +5 %. The asserted front-hit
bands moved to 0.4–1.3× a win in Simulation and 0.5–1.5× in Arcade, and the Extreme career
median floor to 0.5× the start (it lands near 0.6×). The table above predates
this; rerun the test with `--nocapture` for current numbers.

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
- `tests/browser_career.rs`: step-2 guardrails (a new player affords a beater and three busted
  nights; an L2 night escape out-earns an L1 beater bust, a Sport bust costs more than the
  escape at the same level, Simulation busts cost more). `src/profile.rs` unit tests: save
  round trip, version checks, defaults, carry-over rules. The browser career itself is tested
  in `crates/wp_wasm/src/career_tests.rs`.
- `tests/data_tables.rs`: data completeness, embedded copy == `data/`, every mod applies.
- `tests/stores.rs`: in-memory + SQLite stores, sequence conflicts, `SimEvent` round trip.
- `tests/pursuit_wiring.rs`: a real `PoliceSystem` capture settled through the economy (both modes), escape / safehouse / called-off settlement, tracker event-order rules.
- `tests/session.rs`: `EconomySession` replaying event lists recorded from a real
  `PoliceSystem` (`tests/fixtures/*.json`; re-record with `-- --ignored record_fixtures`):
  capture → fine → impound → pound fee grows → release with the same damage → repair bill;
  total-loss capture by mode; escape → bounty → repair paid; unaffordable / quote-only repairs;
  uncaptured total loss towed; races; all-or-nothing and per-account `finish`; auction
  countdown; the police decoder against `wp_police`'s encoder (every level and outcome); a live
  police run settles like its recording. The recorded racer is parked mid-block at the first
  junction no traffic camera covers and keeps speeding every 4 s until a unit starts the
  pursuit (police only send a unit to a civilian report, and the racer's heat decays before
  the night-time responder arrives otherwise).
- `tests/race_kinds.rs`: every contract `RaceKind` maps onto `wp_race`'s race type and stays
  within 15 % of `wp_race`'s relative purse; legality order; outrun recorded as touge.
- `tests/roster_prices.rs`: feel targets on every roster car (table above).
- `tests/resale.rs` (proptest): every roster car × condition × random mods × mode: buy → sell
  or scrap at once returns less than was spent; buy → (insure) → crash → repair (legit or
  crooked) → sell returns less than everything spent, and insurance pays less than the bill;
  the floor car never sells or scraps above what was paid (any cash); v1-save cars use the
  cheapest price; the owner's two repro cases.
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
- The browser career (`wp_wasm::career`) keeps lattice damage per car and repairs it with
  `apply_repair`; the server path still stores no lattice blob. The scripted career scenario
  uses `reference_estimate` (same pricing as `wp_damage`, synthetic severities).
- The career's win rate does not depend on the car (see the roster sweep).
- No `wp_vehicle::metrics` yet, so `preview` uses the built-in estimator (comparative, not
  lap-time accurate).
- Body-kit pieces only become detachable where the lattice already has a detachable group for the
  region; creating new groups is a `gen-lattice` extension (prompt 03).
- Data files have no generated JSON Schemas (economy types live outside `wp_contracts`); they are
  validated by typed loaders with `deny_unknown_fields` and completeness checks.
- CI runs without `--features native`, so the SQLite tests only run locally until CI adds that
  line (requested).
- Career ledger entries (tier bonuses, bets, extra rep) are `Adjustment`s with `career:` /
  `bet:` notes until the contract has typed reasons (`career-progression.md`, request #1). Tier
  rewards are paid when the caller runs `update_progression` (the browser career does it at the
  end of every run and night; the server path does not call it yet).
