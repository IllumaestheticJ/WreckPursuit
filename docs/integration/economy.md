# Economy integration notes

- **Workstream / prompt:** 08 — economy, progression, mods
- **Branch:** `ws/economy`
- **Crates / folders owned:** `crates/wp_economy`, `data/economy/`, `data/mods/`
- **Contract version built against:** 0.3.0

Design write-up, tuning levers and measured numbers: `crates/wp_economy/README.md`.

## Exposes

Everything is a method on `wp_economy::Economy` (immutable data bundle). Mutating calls take
`&mut PlayerAccount` and return `Result<(T, Vec<EconomyEvent>), EconomyError>`; a refused call
never changes the account. Persist with `AccountStore::commit(&account, &events)`.

### Browser career (step 2) — `wp_wasm::career` + `wp_economy::profile`
A single-player career that persists between runs in the browser. The career owns the account
between runs; during a run the session's `PlayerRun` holds it and the career takes it back when
the run ends.

| wasm export | Notes |
|---|---|
| `career_new(mode)`, `career_load_json(text)`, `career_save_json()`, `career_exists()` | save = `CareerProfile` JSON (versioned; `migrate` refuses newer saves). During a run `career_save_json` includes the live account and the car's current damage |
| `career_view_json()` | `CareerView`: cash / debt / rep / bounty / net worth, carried heat + level, cars (condition, value, sale + scrap price, mods, insurance, `RepairAll` quote, priced repair lines, pound status, PI / mass / 0–100 / top speed), dealer (street roster, New/Used/Worn prices), floor car, insurance tiers, district labour, stats, ledger tail |
| `career_mods_json(uid)` | mod tree for the car's class and mode: installed, allowed + reason, same-line replacement, stats after the change |
| `career_start_run(scenario_json)` | any live scenario (free drive, city chase, a race): entity 0 becomes the active car with its mods (overlay data, same car id), saved lattice restored, career mode, carried heat as `police_heat`; ends a career run in progress first |
| `career_end_run()` | `RunClose { end: Free/Captured/Wrecked, carried_heat, cash, ledger }`; stores the car's damage, applies carry-over, one night passes. Replacing the session (`init`, another start) also ends it |
| `career_run_active()` | |
| `career_buy_car(def, cond)`, `career_floor_car()`, `career_sell_car(uid)`, `career_set_active(uid)` | dealer = roster minus `debug_*` and police |
| `career_install_mod(uid, id)`, `career_uninstall_mod(uid, id)` | a mod that changes the lattice structure (cages) is refused while the body is dented |
| `career_quote_json(uid, req)`, `career_repair(uid, req)` | `req = { option?: RepairAll / {RepairSelected:[items]} / Scrap, district?: DistrictId, shop?: Legit/Crooked }`; the stored lattice is repaired with `DamageSim::apply_repair` (full repair = pristine body); a legit shop reporting a wanted car raises carried heat |
| `career_release(uid)`, `career_next_night()` | pound release; a night without driving (auction countdown, premiums) |
| `career_insure(uid, tier)` | `"basic"`, `"full"`, `""` = drop the policy |

Rules: each finished run is a night; capture / wreck clears heat; a free end carries heat × 0.5
(≤ 45, < 5 forgotten); bounty, rep, debt and the pound clock live in the account. Career runs
keep the career's damage mode (`set_damage_mode` is refused). Lattice blobs are
`wp_damage::FullDamageSnapshot` JSON text per car uid (exact round trip).

Testbed (`apps/testbed-web`): `garage` button and a `career` box (on by default) in the
controls: with it, **free drive** and **city chase** start career runs; the car selector and
`window.__wp.startFreeDrive(car, …)` / `startCityChase(car, …)` stay sandbox runs. Saved to
`localStorage["wp.career.v1"]` (try/catch everywhere; an unreadable save is kept under
`wp.career.v1.unreadable`) after every garage action, every settlement and every 20 s of a run;
export / import as a file from the garage. The end-of-run panel gets a **Garage** button on
career runs.

### End-of-run glue for `wp_sim` (`wp_economy::session`) — round 6b
| Call | Notes |
|---|---|
| `EconomySession::new(SessionConfig::new(mode))` | `SessionConfig { mode, shop, repair: RepairPolicy::{PayIfAffordable (default), QuoteOnly}, phase }` |
| `bind(vehicle: EntityId, player, car: CarUid)` | at spawn; unbound vehicles (traffic, police) are never settled |
| `observe(&SimEvent)` | every drained event; reads `TrafficViolation`, `TotalLoss`, police `WantedLevelChanged` / `PursuitEnded` (0.03 µs/event) |
| `observe_police(&PoliceSignal)` | for typed v0.3.0 events / already-decoded police events |
| `set_day_phase(phase)`, `add_race_result(vehicle, RaceSetup, RaceResult)`, `mark_towed(vehicle)` | |
| `is_captured(vehicle)`, `was_wanted`, `wanted_level`, `pending_pursuits()` | end the run on a capture |
| `finish(&econ, &mut acct, &[DamageReport], &mut rng) -> RunResult` | all-or-nothing, per account: damage (towing) → races → pursuits (fine / bail / impound / seizure / destruction, escape bounty) → repair bill. `RunResult { cash_before, cash_after, payouts, fines, bail, towing, repairs_paid, rep_delta, bounty_delta, races, pursuits, cars: [CarRunResult { status, towed, repair_quote, repair_order, impound }], events }` |
| `PoliceSignal::from_sim_event(&SimEvent)` | decodes today's opaque police events (kinds 2 and 4); the one function that changes for typed v0.3.0 police events |
| `Economy::impound_status(&acct, car) -> ImpoundStatus` | `{ days, level, fee, fee_per_day, days_until_auction, affordable }` |

Demo wiring (single player): bind the player's vehicle at spawn, `observe` every drained event,
end the run when `is_captured` (or the player quits / reaches the garage), build the
`DamageReport` from `wp_damage` (estimate priced with `econ.price_table_for(car)`), call
`finish`, show `RunResult`, `store.commit(&acct, &result.events)`. If `repair_order` is `Some`,
apply it to the stored lattice (full reset today). An impounded car comes back through
`retrieve_from_impound` with its damage intact.

Switching to typed v0.3.0 events: replace the body of `PoliceSignal::from_sim_event` with a
match on the typed police variant (and take `level` from `PursuitEnded` when it has it); the
session API does not change.

### Data / setup
| Item | Notes |
|---|---|
| `Economy::embedded()` | data compiled in (browser, tests) |
| `Economy::load(&Path)` | reads `data/` (server) |
| `Economy::mode_table(DamageMode)` | the `data/modes` table used for `repair_cost_scale` |

### Ledger API (`wp_economy::ledger`)
| Item | Notes |
|---|---|
| `PlayerAccount { player, cash, rep, bounty, cop_rank, garage: Vec<OwnedCar>, active_car, heat_history, stats, day, .. }` | persistent per-player state |
| `OwnedCar { uid: CarUid, def_id, class, base_price, mods, damage: DamageSnapshot, location, insurance }` | `DamageSnapshot` keeps `lattice_state: Vec<u8>` (opaque, from `wp_damage`) + `HandlingEffects` + outstanding `RepairEstimate` |
| `EconomyEvent { player, seq, day, reason: EventReason, car, cash_delta, rep_delta, bounty_delta, cop_xp_delta, cash_after }` | gap-free `seq` per account |
| `PlayerAccount::account_id()`, `EconomyEvent::account_id()`, `AccountStore::{load_account, account_events_since}`, `account_id_of` / `player_of` | durable `AccountId` view of the ledger (round 8; the ledger key is still `PlayerId` with the same number, see request #7) |
| `EconomyEvent::to_sim_event()` / `from_sim_event()` | `SimEvent::EconomyEvent { kind: reason.kind_code(), payload: JSON }` for the client feed |
| `trait AccountStore { load, commit, events_since }` | `InMemoryStore`; `sqlite::SqliteStore` behind feature `native` (server only) |
| `Economy::open_account(PlayerId)`, `end_of_day(&mut acct)` | the in-game day is advanced by the server, never wall-clock |

Events other systems should react to: `EventReason::InformantTip { heat_reduction }` and
`EventReason::ShopReported { district, level }` (police heat), `BountyCleared` (heat reset after
capture), `CarImpounded/CarSeized/CarDestroyed` (despawn / do not spawn that car).

### Garage API
| Call | Notes |
|---|---|
| `buy_car(&mut acct, &CarSpec, Condition)`, `sell_car`, `market_price`, `sale_price` | `CarSpec::from(&VehicleDef)`; police classes not for sale |
| `issue_floor_car(&mut acct)` | economy floor: always obtainable for `min(price, cash)` |
| `install_mod(&mut acct, car, mod_id, mode)`, `uninstall_mod` | whitelist, requirements, mode, line replacement + 40 % refund |
| `preview(&VehicleDef, &LatticeDef, &[mod ids], Option<DamageMode>) -> Preview` | modified `VehicleDef` + `LatticeDef` (bases untouched), `ToolLoadout`, cosmetics, `DetachableRequest`s, `class_index`, bracket, mass, `est_0_100_s`, `est_top_speed_mps` |
| `class_index(&VehicleDef)`, `performance(&VehicleDef)` | lobby brackets |
| `buy_insurance(&mut acct, car, tier)` | premium daily via `end_of_day` |
| `car_value`, `current_value`, `net_worth` | valuation |

**To spawn an owned car**, the sim should: `preview(base_def, base_lattice, &car.mods, Some(mode))`
→ use `applied.vehicle` / `applied.lattice`; restore `car.damage.lattice_state` into the
`DamageModel`; apply `car.damage.handling`; give the vehicle `applied.tools`.

### Repair billing
| Call | Notes |
|---|---|
| `price_table(class, car_value)` / `price_table_for(&OwnedCar)` | contract `PriceTable` to pass to `DamageSim::repair_estimate` |
| `record_damage(&mut acct, &DamageReport)` | end of session: health, total_loss, handling, estimate, lattice blob, `wanted_during_session`, `towed` (towing is a penalty) |
| `quote_repair(&acct, car, &RepairOption, &ShopContext, mode) -> RepairQuote` | `RepairAll`, `RepairSelected(items)`, `DriveDamaged`, `Scrap`; district + crooked/legit labour; insurance. `ShopContext.district: DistrictId` (round 8); `district_from_key(&str)` parses names and the old keys (`default` → Residential, `Docks` → Port, `Hills` → Suburb) |
| `repair(...) -> RepairOrder` | the damage system must apply the order to the stored lattice and send a fresh `DamageReport` (`towed: false`) |
| `repair::reference_estimate(&PriceTable, scale, &[(RepairItem, severity)])` | formula stand-in for `wp_damage` |

### Payouts
| Call | Notes |
|---|---|
| `race_setup(RaceKind, Legality, class, route_length_m, phase, mode) -> RaceSetup` | races from `wp_race` (R5, round 8): `RaceSetup { kind: Option<RaceKind>, legality, .. }` (both serde-default); `[race_kinds]` picks the race-type row + purse/buy-in multipliers, `[legality]` scales them. `race_type_for(&kind)`, `effective_race_type(&setup)` (the type recorded in the ledger) |
| `entry_fee(&RaceSetup)`, `base_purse`, `race_payout(&RaceSetup, &RaceResult)` | pure |
| `enter_race(&mut acct, &RaceSetup, car)` | debt blocks any buy-in > 0 (free `RaceType::Meet` allowed) |
| `settle_race(&mut acct, &RaceSetup, car, &RaceResult)` | purse share, clean bonus, rep, bounty while wanted |
| `check_pink_slip`, `settle_pink_slip(winner, wcar, loser, lcar, mode, PinkSlipConsent)` | Simulation only by default; returns both accounts' events |
| `buy_informant_tip(&mut acct, tier) -> heat_reduction` | |

### Capture handling
| Call | Notes |
|---|---|
| `handle_capture(&mut acct, &CaptureContext, &mut Rng) -> CaptureOutcome` | fine + bail (may create debt), rep loss, bounty cleared, insurance voided, `CarFate::{Impounded, Seized, Destroyed}` by mode |
| `handle_escape(&mut acct, level, phase_bounty)` | bounty + rep |
| `retrieval_fee`, `retrieve_from_impound` | fee grows daily; auction after 10 days |
| `award_cop(&mut acct, CopAction)`, `cop_level(xp)` | captures, assists, clean minutes, civilian hits, self-wrecks, risk-budget overspend |

### Pursuit wiring (draft, drops in when police events are typed in v0.3.0)
`wp_economy::pursuit` turns a police pursuit ending into the capture / escape calls above
without depending on `wp_police`:

```rust
// once per instance, next to the sim's event drain (wp_sim or the server)
let mut tracker = wp_economy::PursuitTracker::new();
for ev in sim.drain_events() {
    tracker.observe_sim_event(&ev);                       // counts TrafficViolation per vehicle
    if let Some(sig) = PoliceEvent::from_sim_event(&ev).as_ref().and_then(police_signal) {
        if let Some(res) = tracker.observe_police(&sig) { // a pursuit ended
            if let Some((acct, car)) = owner_of(res.racer) {   // EntityId -> account + CarUid
                let (settled, events) = econ.settle_pursuit(acct, car, &res, mode, phase, &mut rng)?;
                store.commit(acct, &events)?;
            }
        }
    }
}
```

- `police_signal` is the 15-line adapter in `crates/wp_economy/tests/pursuit_wiring.rs`
  (`WantedLevelChanged` / `PursuitEnded` → `PoliceSignal`, exhaustive over `PursuitOutcome`).
  With typed contract events it becomes a plain match.
- Settlement: `Captured` → `handle_capture`; `CapturedTotalLoss` → `handle_capture` with
  `total_loss` (Simulation: car destroyed; Arcade: impounded wrecked); `Escaped` / `Safehouse` →
  `handle_escape` × the day phase's bounty multiplier; `CalledOff`, or ending with no wanted
  level → nothing.
- The level used is the level **at** the end: `wp_police` emits `PursuitEnded` before the
  `WantedLevelChanged` that resets it, and the tracker relies on that order.
- Fines count the racer's `TrafficViolation`s since their last capture.
- Tested end to end against a real `PoliceSystem` (heat → wanted → total loss →
  `CapturedTotalLoss` → settled, both modes).

### Tuning harness
`career::simulate_career(&Economy, &CareerConfig)` — scripted multi-night career (deterministic
per seed). `apps/simcli` (workstream 10) may wrap it as an economy scenario.

## Needs

| From | What | Workaround now |
|---|---|---|
| 07 net / 12 lead | Call sites: race results (`RaceSetup` + `RaceResult` per racer, server-validated), session end `DamageReport`s, capture/escape, garage UI messages; persist with `SqliteStore` (enable `wp_economy/native` in `apps/server` only) | tests drive the API directly |
| 06 police (E) | **Round 8, stationary capture (lead item 5; decide whether intended).** Found while restoring `live_police_run_settles_like_the_recording` (3 am, racer parked, heat set directly, `internal_driving: true`, seed 8, small procedural city): **(a)** a violation a traffic camera witnesses only sets `last_known`; no report is filed and nobody is sent there, so a wanted racer who parks under a camera is never visited (the old recording position (-605, -605) is camera-covered). A civilian report is dispatched (`report_dispatch_delay_s.night` 50 s), but the racer's heat decays to None ~18 s after the last violation, so the responder arrives at a racer who is no longer wanted. **(b)** With the racer parked on a junction centre (-495, -605), the pursuing unit's Shadow "close in for the arrest" (`want_gap = 8.0` along the track, `pursuit.rs` ≈ line 833) stops it 16 m away in a straight line, just outside `capture.unit_radius_m = 15`, so the stopped racer is never captured (pursuit escalates to L3 instead). Suggest: close in by straight-line distance (or to `unit_radius_m − 3`) when the racer is stopped, and dispatch a unit to a camera hit on a wanted, unpursued racer. My test now parks mid-block off-camera and keeps offending, so it passes either way; tell me if you change (a)/(b) and I will move the racer back to the junction to cover it. | test racer placed where police reach it |
| 06 police | ~~`set_heat` emits no `WantedLevelChanged`~~ done (it emits now; v0.3.0 `PursuitEnded` carries `level`). | — |
| 06 police | ~~Capture/escape payload~~ covered by `PursuitEnded` + `WantedLevelChanged` (pursuit wiring above). **New for v0.3.0:** `PursuitEnded` should name the capturing unit and assisting units (`capturer: Option<UnitId>, assists: Vec<UnitId>`) so `award_cop` can pay player cops; and the event order (ended before the level reset) should stay as it is or carry `level` in the event. Consume `InformantTip` / `ShopReported` / `BountyCleared`. Feed the risk-budget overspend for `CopAction::RiskBudgetOverspent`. Read `ToolLoadout` (radar/scanner/fake plates) | `CaptureContext` built by hand in tests |
| 03 damage | (a) ~~unscaled `parts_cost` / `labour_hours`~~ done in v0.2.0; (b) an API to serialize / restore lattice state (`Vec<u8>`) and to apply a `RepairOrder` (restore listed items / full reset); (c) `gen-lattice` extension: create detachable groups for body-kit `DetachableRequest`s | `reference_estimate`; lattice blob stays empty |
| 02 vehicle | ~~Tire files~~ landed; `market.tire_grip` is now each file's `lateral.d` / street's (a test requires an entry per `data/tires/` file). Optionally `wp_vehicle::metrics` to replace the estimator; consume `ToolLoadout.run_flat`, `reinforced_cooling`, `nitrous` | estimator + grip table |
| 09 cars | ~~`junkyard_beater`~~ landed (a test checks it matches `market.toml [floor_car]`). **Price retune (round 6b):** `hakone_gt` 48 000 → **40 000** (PI 494 vs Sperber 32's 500 at 36 000; its front-hit repair is 1.5× a win, the highest of the Sport cars); `ironside_440` 38 000 → **26 000** (PI 330, slower than the 22 000 Kestrel TS and the 28 000 Shiro Turbo). Both are listed as known outliers in `tests/roster_prices.rs::pricier_cars_are_not_slower`; that test fails once they are fixed until they are removed from `KNOWN`, so tell me (or remove them yourself — test-only change). Every other price is fine as is. | known-outlier list |
| 15 race (L) | **R5 agreed mapping (round 8):** the economy prices every `RaceKind` itself from `stakes.toml [race_kinds]` (same type mapping as your `bridge::economy_race_type`: Outrun → Touge, TimeAttack → Circuit, Checkpoint / Cannonball / Delivery → Sprint; multipliers keep your `data/events/stakes.toml [kinds]` purse ratios within 15 %, checked by `tests/race_kinds.rs`, which reads your file) and `[legality]` (sanctioned ×0.8 purse / ×1.2 buy-in, illegal daytime ×4.5 ≈ 2.5× a night street race). Your `Stake.purse` stays the lobby's advertised number; the payout is the economy's. I added `kind: Some(kind), legality` to the `RaceSetup` literal in `bridge::economy_setup` (compile fix); you may replace the body with `econ.race_setup(..)` if you have an `Economy` there. If you change the kind → race type mapping or the relative `base_purse`s, `race_kinds.rs` fails: tell me and I retune. Still open from R5: charge `cargo_damage_cost` and apply `CrewCut` at the call site (server) — economy calls exist (`Adjustment` / a new reason if you want one: say which). | — |
| 10 harness | CI line `cargo test -p wp_economy --features native` so the SQLite store is tested in CI | run locally |

### Needs (step 2)
| From | What | Workaround now |
|---|---|---|
| I (harness) / lead | A replay of a career run whose car has mods or started damaged does not verify (replay rebuilds the stock, pristine car). Proposal: scenario `EntitySpec` gains optional `car_def` / `lattice_def` overrides and an initial `damage: FullDamageSnapshot`, applied by `build_sim`; or a replay op | stock, undamaged career cars replay fine; documented |
| L (race menu) | Start races from the menu with `career_start_run(scenario_json)` when the career is on, so races use the career car; race payouts then flow through the run's `PlayerRun` (needs a race-result call there, lead) | free drive / city chase only |
| B (damage) | Contract request #9 (`DamageSnapshotExt` into `DamageSnapshot`) would let the career store the contract type instead of `FullDamageSnapshot` | `FullDamageSnapshot` JSON |

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| 1 | Promote the economy event payload: add `SimEvent::Economy(EconomyEvent)` (or a typed `EconomyEventKind` enum) with the shape of `wp_economy::ledger::EconomyEvent` / `EventReason`, replacing the opaque `{ kind, payload }` | the client feed / net layer need typed events (amounts, reasons, car ids); today they get JSON bytes | 07 net, 04 testbed UI, 12 | accepted → v0.3.0; final variant list below |
| 2 | `RepairEstimate` promotion refinements: document that `RepairLine::parts_cost` and `labour_hours` are **unscaled** and only `total` includes `repair_cost_scale`; add `PriceTable::labour_hours: BTreeMap<RepairItem, f32>` (per-item hours) with `labour_hours_per_item` as the fallback | billing re-prices labour per district / shop and must not double-apply the mode scale; a roof costs more labour than a mirror | 03 damage, 08 | **done in v0.2.0**; `price_table` fills `labour_hours` + `repair_parts_fraction` from `prices.toml` |
| 3 | `RepairEstimate::health: f32` and `total_loss: bool` (or a `DamageSummary` returned next to the estimate) | the economy needs both at session end; today `DamageReport` carries them separately | 03, 08, 12 | deferred → v0.3.0 (`DamageSummary`) |
| 4 | `DamageSim::{save_state() -> Vec<u8>, restore_state(&[u8]), apply_repair(&[RepairItem], full: bool)}` | `DriveDamaged` must carry the lattice across sessions; partial repairs must restore only the paid items | 03, 08, 12 | accepted → v0.3.0 (03 implements) |
| 5 | Move `WantedLevel` from `wp_police` into `wp_contracts` | economy, net and police all key tables by it; today `wp_economy` depends on `wp_police` just for this enum | 06, 07, 08 | **done in v0.2.0**; `wp_economy` now uses `wp_contracts::WantedLevel` and no longer depends on `wp_police` |
| 6 | Optional: `DistrictId` type in contracts (prompt 05 ids) | repair labour is keyed by district name strings (`prices.toml [district_labour]`) | 05, 08 | `DistrictId` in v0.3.0. **Round 8: economy side done** — `[district_labour]` is keyed by every `DistrictId`, the ledger's `ShopReported` carries a `DistrictId`, and the contract string is its variant name (`"Core"`, `"Port"`, ..). **Lead:** `EconomyReason::ShopReported.district` can become `DistrictId` (same JSON for every value the economy emits); then I drop `district_key` / the parse in `from_contract`. |
| 7 | Ledger keyed by `AccountId` (round 8 item 3) | the contract event already says `account: AccountId` | 08, 18 meta, 07 net | **Not cheap, proposed for a coordinated step.** `PlayerAccount.player` / `EconomyEvent.player` / `AccountStore::{load, events_since}(PlayerId)` are used by `apps/meta` (two `AccountStore` impls, routes, client, auth, and the JSON stored in its `econ_accounts` / event tables and served by its HTTP API) and `wp_net`'s server. Plan: rename the fields to `account: AccountId` with `#[serde(alias = "player")]` (old JSON still loads), switch the trait to `AccountId`, and fix meta + net in the same commit (mechanical: `PlayerId(x)` → `AccountId(x)`). Needs O's (meta) and F's (net) OK since it touches their crates; until then the additive accessors above give everyone the durable id. |

### Typed `EconomyEvent` for v0.3.0 — final variant list (request #1, with 07's NET-4)

Proposal: replace `SimEvent::EconomyEvent { kind, payload }` with `SimEvent::Economy(EconomyEvent)`,
where the contract `EconomyEvent` is exactly today's `wp_economy::ledger::EconomyEvent`:

```rust
pub struct EconomyEvent {
    pub account: AccountId,   // durable account id (07 NET-5); today `player: PlayerId`
    pub seq: u64,             // gap-free per account
    pub day: u32,             // in-game day
    pub reason: EconomyReason,
    pub car: Option<CarUid>,  // CarUid(u32), unique per account
    pub cash_delta: i64, pub rep_delta: i64, pub bounty_delta: i64, pub cop_xp_delta: i64,
    pub cash_after: i64,
}
```

`EconomyReason` variants (stable `u16` codes already used as `kind`; keep them):

| Code | Variant | Fields |
|---|---|---|
| 1 | `AccountOpened` | — |
| 10 | `StakeCharged` (07 NET-4; today `RaceEntryFee`) | `race: RaceType` |
| 11 | `RacePayout` (07 NET-4; today `RacePurse`) | `race: RaceType, placement: u8` |
| 12 | `CleanRaceBonus` | — |
| 13 | `RaceRep` | `placement: u8` (0 = DNF) |
| 14 / 15 | `PinkSlipWon` / `PinkSlipLost` | `def_id: String` |
| 20 | `RepairPaid` | `lines: u32, labour_multiplier: f32` |
| 21 / 22 | `InsuranceClaim` / `InsuranceClaimDenied` | `covered: i64` / — |
| 23 | `DriveDamaged` | — |
| 24 | `CarScrapped` | `def_id` |
| 25 | `ShopReported` | `district: DistrictId (String today), level: WantedLevel` |
| 26 | `DamageRecorded` | `health: f32, total_loss: bool` |
| 30 / 31 | `Fine` / `Bail` | `level` / — |
| 32 | `CaptureRepLoss` | `level` |
| 33 / 34 | `BountyCleared` / `BountyGained` | — |
| 35 | `Towing` | — |
| 36 / 37 | `CarImpounded` / `ImpoundRetrieved` | — / `days: u32` |
| 38 / 39 / 40 | `CarAuctioned` / `CarSeized` / `CarDestroyed` | `def_id` |
| 41 | `Escape` | `level` |
| 50 / 51 | `CopReward` / `CopXp` | `level` / `reason: CopXpReason { Capture, Assist, CleanDriving, CivilianCollision, SelfWreck, RiskBudgetOverspent }` |
| 52 | `InformantTip` | `tier: String, heat_reduction: f32` |
| 60 / 61 / 62 | `CarPurchased` / `CarSold` / `FloorCarIssued` | `def_id` |
| 63 / 64 | `ModInstalled` / `ModRemoved` | `mod_id: String` |
| 70–73 | `InsurancePurchased` / `InsurancePremium` / `InsuranceLapsed` / `InsuranceVoided` | `tier` / — |
| 999 | `Adjustment` | `note: String` (admin / harness only) |

Police side, same bump: move `PursuitOutcome` into contracts (economy mirrors it as
`wp_economy::PursuitEnd` today) and add `level: WantedLevel`, `capturer: Option<UnitId>`,
`assists: Vec<UnitId>` to `PursuitEnded`.

Also needed in contracts for this: `RaceType { Sprint, Circuit, Drag, Touge, Meet }` (today in
`wp_economy::payouts`; 15 `wp_race` already uses it), `CarUid(u32)`, `CopXpReason`. When the bump
lands I rename `RaceEntryFee`/`RacePurse` to `StakeCharged`/`RacePayout`, switch `player` to the
durable account id, and keep `to_sim_event`/`from_sim_event` as thin wrappers for one release.
Police / net consumers: `InformantTip`, `ShopReported`, `BountyCleared` change heat;
`CarImpounded`/`CarSeized`/`CarDestroyed` mean the car must not spawn.

## Changes outside my crates
**Step 2 (career), authorized minimal glue:**
- `crates/wp_wasm` (C): new `src/career.rs` + `src/career_tests.rs` (mine); `session.rs`:
  `Session::new_with_data` (cars read through a data overlay), `is_career` / `begin_career` /
  `end_career` / `sim_before_start_mut`, `set_mode` refused in career runs; `lib.rs`: `install`
  ends a replaced career run (`career::absorb`), `with_session_opt`, module lines; `run.rs`
  (lead): `PlayerRun::for_career_car`, `was_wanted`, `car`, `RunReport::escape_cash`;
  `run_tests.rs`: the escape test now expects escape cash; `Cargo.toml`: `wp_damage` (already in
  the tree via `wp_sim`).
- `apps/testbed-web` (C): `index.html` (`career` box, `garage` button), `main.ts` (career
  load/save, garage mount, career runs, autosave, `__wp.{garage, careerApi, openGarage,
  startCareerRun, persistCareer}`), `wasm/sim.ts` (`career*` wrappers), `ui/runEnd.tsx`
  (Garage button, escape cash in the toast), `e2e/run.mjs` (career loop check).
- `apps/ui-kit` (M): new files only, `src/garage/{CareerGarage.tsx, types.ts, store.ts,
  garage.css}`; they use `ui/kit.tsx`, `ui/format.ts` and `ui/theme.css`, nothing else.


Round 12b (minimal fixes for the new `OwnedCar::purchase_price` field / save v2):
- `apps/testbed-web/e2e/run.mjs` (C, my career block): the bust check expects save `version === 2`.
- `crates/wp_wasm/src/run.rs` (lead): the demo's handed-out car gets `purchase_price: Some(0)`
  (it was free, so it resells for its mods only — no money from nothing).
- `crates/wp_wasm/src/garage.rs` (C): the throwaway quote car gets `purchase_price: None`
  (only quoted, never sold).

**By the lead in `wp_economy` (`lead/demo-endrun`, 2026-10-19):** `EconomySession::set_mode(mode)` (the testbed switches damage mode mid-run; `finish` and quotes use it). Additive, no behaviour change. The demo calls the session from `crates/wp_wasm/src/run.rs` with `RepairPolicy::QuoteOnly`, then `retrieve_from_impound` + `repair(RepairAll)` on "Repair & continue".

Round 8 (minimal compile fixes for new fields, nothing else):
- `crates/wp_race/src/bridge.rs` (L): `economy_setup` fills `RaceSetup { kind: Some(kind), legality, .. }` (the `_legality` parameter is now used) and its doc line.
- `apps/meta/tests/bots.rs` (O): `RaceSetup` literal gets `kind: None, legality: Default::default()`.
- `crates/wp_wasm/src/garage.rs` (C): the overlay's `district` string is parsed with `wp_economy::district_from_key` (`"default"` still works; unknown names are an error).

Before round 8: none. (The previous stub's public API in `wp_economy` changed — `PlayerAccount::garage` is now
`Vec<OwnedCar>`, `AccountStore` is `load -> Result<Option<_>>` / `commit` / `events_since`,
`repair_bill` is replaced by `quote_repair`/`repair` — no other crate used it.)
Dependencies: `wp_contracts`, `wp_math`, `serde_json`, `toml`, `thiserror` (all wasm-clean;
`wp_police` dropped in v0.2.0); `rusqlite` (bundled) optional behind `native`; dev-only
`proptest` and `wp_damage` (a test keeps `repair::reference_line` equal to `wp_damage`'s pricing
constants).
Versions for `rusqlite`/`proptest` are pinned in the crate's `Cargo.toml`, not the workspace
table (lead may move them).

## Perf numbers
(Round 8 changes add one `BTreeMap` lookup per purse / fee; not re-measured.)

| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `preview`, 7 mods | cloud container, release | 167 µs | — | not per-frame |
| `preview`, welded cage | cloud container, release | 248 µs | — | not per-frame |
| `enter_race` + `settle_race` | cloud container, release | 0.4 µs | — | not per-frame |
| `EconomySession::observe`, per event | cloud container, release | 0.03 µs | — | runs in the event drain |
| `EconomySession::finish` (capture + impound) | cloud container, release | 1.3 µs | — | not per-frame |
| `handle_capture` | cloud container, release | 0.2 µs | — | not per-frame |
| 20-night career (60 races) | cloud container, release | 168 µs | — | — |

## Known gaps
- Lattice blobs and partial repairs wait on `DamageSim::{save_state, restore_state, apply_repair}`
  (v0.3.0). Real estimates come from `wp_damage` once the sim calls `record_damage`;
  `reference_estimate` now prices with exactly `wp_damage`'s formula for tests and the career.
- The career's win rate does not depend on the car, so its roster sweep overstates how much
  expensive cars lose in Simulation (README, "Prices against the car roster").
- `EconomySession` has no persistence of its own: a run that crashes before `finish` loses its
  pending settlements (the server should `finish` on disconnect).
- Body kits only reuse existing detachable groups (03 `gen-lattice` extension needed).
- Economy data has no generated JSON Schema (types live outside contracts); typed loaders with
  `deny_unknown_fields` + completeness checks validate it.
- 20-night tuning numbers come from the scripted career with synthetic damage; retune once real
  damage estimates flow (expect Simulation repair costs to move the most).

## Lead feedback (2026-10-19)

**Merged** (`f75248f`). All tests green, including
`cargo test -p wp_economy --features native` (SQLite store). `rusqlite` stays behind the
off-by-default `native` feature and does not reach `wp_wasm` (checked with `cargo tree`).

Lead change in your area: `src/mods.rs` (~line 1119) used `f32::powi(2)`; the determinism lint rejects
it (`powi` is not guaranteed bit-identical across targets), so it is now plain multiplies. Use
`x * x` in sim-reachable code.

Contract requests (decisions; implemented together in v0.2.0 unless noted):
| # | Decision |
|---|---|
| 1 typed economy events | **Accepted.** `SimEvent::EconomyEvent { kind, payload }` becomes a typed enum, designed together with 06's police events and 15's race events in one bump. Your `EventReason` is the starting point. |
| 2 unscaled repair lines + per-item labour | **Accepted**: doc says `parts_cost`/`labour_hours` are unscaled and only `total` includes `repair_cost_scale`; `PriceTable::labour_hours: BTreeMap<RepairItem, f32>` with `labour_hours_per_item` as fallback. |
| 3 health/total loss next to the estimate | Deferred until 03 lands; leaning to `DamageSim::summary() -> DamageSummary { health, total_loss }`. |
| 4 save/restore/apply repair | **Accepted**; 03 implements. |
| 5 `WantedLevel` into contracts | **Accepted**; then drop your `wp_police` dependency. |
| 6 `DistrictId` | Deferred until 05 lands. |

Other needs routed: floor car `junkyard_beater` → 09 (in cars.md); tire files → 02 (in vehicle.md);
CI line → 10 (in harness.md). Call sites (race results, session-end `DamageReport`, capture) come
when 07 (server) and 06 (police) merge — nothing to do now.

## Lead feedback (round 6, 2026-10-19)

**Merged** — typed event list, price-table fill, pursuit wiring draft (`PursuitTracker`, `settle_pursuit`). The lead
wires capture / escape into the sim and the testbed next round, once E's typed events land in contract v0.3.0.


## Lead feedback (round 6b, 2026-10-19) — head-start tasks

Contract **v0.3.0** is being applied in parallel by the lead (branch `lead/contract-v0.3.0`). Do not edit `wp_contracts`; build your work so it drops onto v0.3.0 (inherent methods / local types now, trait wiring after). Merge `origin/claude/laughing-newton-0xqr6q` (the lead branch, ahead of `main` until today's gate finishes) before starting. Goal of the project right now: the **single-player greybox demo**: drive in the city, raise heat, get chased by physical police, escape or get caught, and pay the repair bill.

1. Build the glue the lead calls from `wp_sim`: an `EconomySession` (in `wp_economy`) that takes a stream of `SimEvent`s plus a per-car `DamageReport`, and produces the end-of-run result: repair bill, payouts, fines, impound decision, ledger entries. Use today's v0.2.0 event shapes, with your `PursuitTracker`/`settle_pursuit` behind it. Write it so switching to typed v0.3.0 events is a small change. Include tests from recorded event lists.
2. Impound flow end to end: capture → impound fee → release, with the car's damage carried over.
3. Tune prices against H's 13 cars (H flagged first-pass prices).

### Round 6b response (economy)

1. **`EconomySession`** (`crates/wp_economy/src/session.rs`, API table under "Exposes"): sim
   events + per-car `DamageReport` → one `RunResult` (repair bill, payouts, fines, bail, towing,
   impound decision, ledger entries), all-or-nothing, using `PursuitTracker` / `settle_pursuit`.
   Police events are decoded by `PoliceSignal::from_sim_event`, the only place that changes
   for typed v0.3.0 events. Tests replay event lists recorded from a real `PoliceSystem`
   (`tests/fixtures/`, `tests/session.rs`), plus a live run that must settle like its
   recording.
2. **Impound flow end to end**: capture → damage recorded → impounded → `impound_status`
   (fee, daily growth, auction countdown) → `end_of_day`s → `retrieve_from_impound` → the car is
   back with the same lattice blob / handling / estimate → repaired; plus the auction when it is
   never retrieved, and the total-loss capture in both modes.
3. **Prices tuned against the 13-car roster** (`tests/roster_prices.rs`, numbers in the README):
   class bands widened (they were clamping Vireo XS / Kestrel TS and erasing their Used/Worn
   discount), Sport parts 1.9 → 1.75, Truck parts 1.5 → 1.3, Muscle reference value 34 k → 40 k.
   Two car prices are 09's to change (Hakone GT → 40 000, Ironside 440 → 26 000; see Needs).
   Career sweep over every street car: Arcade solvent on all of them.

Also: after the lead merge, `real_police_capture_settles_through_the_economy` failed because
`PoliceSystem::set_heat` no longer emits `WantedLevelChanged` (request to 06 under Needs). Fixed
in my test (heat via real violations) and made robust in the tracker (a capture with no level
seen is billed at L1).


Gate on `ws/economy` after merging main + `claude/laughing-newton-0xqr6q`: fmt, workspace clippy
`-D warnings`, wasm build, `cargo test -p wp_economy --features native` all clean;
`cargo test --workspace` green except **`wp_race` `tests/simcli_races.rs::city_sprint_on_real_roads`**
(index out of bounds in `throttle_at`: the run reports more vehicles than the scenario has
entity specs — probably the physical police cars from the lead merge). Not in my crates; fails
the same way without my commits (no economy code on that path). For 15 / lead.


## Lead feedback (round 8, 2026-10-19)

Contract **v0.3.0 is on `main`** (typed police/race/economy events, damage snapshot/summary, road-network trait, weather `LocalConditions`; see `docs/CONTRACT_CHANGELOG.md`). Workspace tests now take about 3.5 minutes (`lead/test-speed`). Owner playtest items are tracked in `docs/PLAYTEST_LOG.md`.

1. R5: `RaceType` vs contract `RaceKind`. Agree the purse mapping with L (Outrun, Checkpoint, Cannonball, Delivery, TimeAttack).
2. `ShopReported.district` → `DistrictId` keys in `prices.toml`.
3. The ledger is still keyed by `PlayerId`. Move it to `AccountId` if cheap.

5. **Parked test (lead, round 8):** `wp_economy/tests/session.rs::live_police_run_settles_like_the_recording` is `#[ignore]`. Since E round 6, no unit finds a racer that never moves (3 am, heat set directly). Work with E: either police should find such a racer, or the test needs a racer police can reach. Also, the recorded fixtures were converted to v0.3.0 typed events by the lead. Re-record them once the live run works.

### Round 8 response (economy)

1. **R5 `RaceType` vs `RaceKind`** — done on my side; mapping and multipliers above (Needs → 15).
   `RaceSetup` gains `kind` + `legality` (serde-default), `Economy::race_setup` builds one, and
   `stakes.toml [race_kinds]` / `[legality]` price them. Ledger records the kind's race type.
   Tests: `tests/race_kinds.rs` (mapping = `wp_race`'s, relative purses within 15 % of
   `data/events/stakes.toml`, legality order, kinds without a setup unchanged, outrun → touge
   in the ledger).
2. **`ShopReported.district` → `DistrictId`** — `prices.toml [district_labour]` keyed by all six
   `DistrictId`s (Docks → Port, Hills folded into Commercial ×1.1, `default` → Residential ×1.0);
   `ShopContext.district` and the ledger's `ShopReported` are `DistrictId`; the contract field
   can now become `DistrictId` (request #6).
3. **Ledger → `AccountId`** — not cheap (meta's stored JSON + two store impls, net server).
   Added the durable-id accessors; coordinated rename proposed as request #7.
5. **Parked test** — `live_police_run_settles_like_the_recording` is back (not ignored) and
   passes; fixtures re-recorded (v0.3.0 typed events). Root cause, for E (Needs → 06): a parked
   wanted racer is only found through a civilian report, its heat decays before the night
   responder arrives, a camera hit dispatches nobody, and the Shadow arrest gap stops the unit
   16 m from a racer on a junction centre (capture radius 15 m). The test racer now parks
   mid-block at the first junction no camera covers and keeps speeding every 4 s until a unit
   starts the pursuit (capture at ≈ 134 s; total-loss capture both settle like the recordings).
   `recorded_capture_arcade_impounds_bills_and_keeps_damage` now counts the speeding tickets in
   the fixture instead of assuming one.


Gate on `ws/economy` after merging `origin/main` (round 8): `cargo fmt --check`, workspace clippy
`--all-targets -D warnings`, `cargo test --workspace` (all green, no failures),
`cargo test -p wp_economy --features native`, wasm build of `wp_wasm` (no `rusqlite` in its tree).


## Step 2 response (economy, 2026-10-19) — persistent career in the browser

1. **Persistence:** cash, ledger (last 200 lines), owned cars with full lattice damage
   (`FullDamageSnapshot` via `save_full`/`restore_full`), mods, insurance, impound status,
   carried heat, bounty / rep / debt, career and lifetime stats. `localStorage` (versioned key,
   try/catch, unreadable saves kept aside, migration stub in `store.ts` for keys and in
   `wp_economy::profile::migrate` for the JSON) plus export / import as a file. Free drive and
   city chase start career runs with the career car, its mods and its saved damage; the
   end-of-run settlement goes into the career.
2. **Garage screen** (`apps/ui-kit/src/garage/CareerGarage.tsx`, the testbed's `garage` button):
   car list with condition and value; repair all or per item at a district (labour ×) legit or
   crooked shop, scrap; dealer with New / Used / Worn prices in the class bands and the floor
   beater; sell; mod tree (performance / durability / tool / cosmetic) with PI, mass, 0–100, top
   speed, grip and brake deltas, install / swap / remove; pound with fee, fee per night and
   auction countdown, release or "let a night pass"; insurance none / basic / full; ledger and
   stats; new career (Arcade / Simulation).
3. **Economy sanity** (`tests/browser_career.rs`, `cargo test -p wp_economy --test browser_career -- --nocapture`):
   start cash 4 000 → **4 500**; worn beater 1 800 leaves 2 700 ≥ three busted L1 nights in it
   (≈ 736 each; L2 ≈ 1 096). New `[bounty.escape_cash]`: night escape L1 455 / L2 1 040 /
   L3 2 080. Arcade busts: Sport L2 1 806, Sport L3 2 586; Simulation Sport L3 4 413. So one L2
   escape out-earns an L1 beater bust, but a bust in a real car costs more than the escape at the
   same level. 20-night career still solvent in Arcade (median 63 447 from 22 000, 0/64 in debt)
   and marginal in Simulation (median 23 782).
4. **Tests:** `crates/wp_wasm/src/career_tests.rs` (10, native): a run uses the career car and
   carries its damage over (restored model == saved); save → load → same profile and stable
   re-save (also through the wasm exports); full repair → pristine body bit-for-bit in the next
   run; partial repair == `apply_repair(items)` on the saved model, the rest one by one ends
   pristine; buy / sell / mod flows (prices, refunds, stat deltas as promised, modded car driven,
   cage refused while dented, durability adds mass and beams); floor car; capture → garage →
   pound fee grows a night → release → repair → new run with the repaired car; auction; heat
   carry; mode lock. Browser e2e (`apps/testbed-web/e2e/run.mjs`, "career:" lines): garage →
   buy beater → city chase → busted → Garage → saved → reload → release → repair all → free
   drive with the repaired car → export/import round trip.

Known gaps: replays of modded / pre-damaged career runs (Needs → I); races in the career (L's
menu); escape payouts are the only career income until races land; the dealer's car names come
from `display_name` (analog names); the in-run HUD shows the run account, not net worth.

Gate on `ws/economy` (step 2, after merging `origin/main`): `cargo fmt --check`, workspace clippy
`--all-targets -D warnings`, `cargo test --workspace` (all green, determinism lint 0 errors),
`cargo build --target wasm32-unknown-unknown -p wp_wasm`, wasm-pack release build, `tsc` in
`apps/testbed-web` and `apps/ui-kit`. **web-e2e:** every check up to "pursuit banner" passes,
then headless Chromium's renderer crashes ("Target crashed") at the lead's end-of-run block —
**the same crash on unmodified `origin/main` in this container** (SwiftShader, after many sessions
in one page; for C / lead). Run on a fresh page, the lead's end-of-run block and all four
"career:" checks pass (capture → Repair & continue; beater bought $4 500 → $2 700, chase with the
career car; busted → garage, saved v1 with the car impounded; reload → release $2 340 → $2 150 →
repair, health 1; new run with the repaired car, export → import identical).

## Lead feedback (round 12b, 2026-10-20) — owner playtest: buy-low / sell-high exploit
Base: `origin/lead/round12` (not main). Owner: "I can buy a used car and sell it for more right now."
Cause (lead read of `wp_economy::garage`): `sale_price` pays `dealer_buy_frac (0.7) × base_price`
regardless of the condition the car was bought in, while `market_price` charges
`base_price × condition` (Used 0.8, Worn 0.6). Buying a Worn car (0.6) and selling it at once
returns 0.7 → +17 % profit. The floor car is worse: price 1 500, base 3 000 → sells for 2 100.
1. Selling must never return more than was paid for the same car in the same state. Record the
   purchase condition (or price paid) on `OwnedCar` and value resale from it (e.g. 0.7 × market
   price in the bought condition), with mods and damage as now. Floor car: resale ≤ its price.
   If this needs a `wp_contracts` field, write the request below and keep a local field meanwhile.
2. Property test: for every roster car × condition × mod set, buy → sell immediately ≤ cost; and
   buy → repair → sell ≤ cost + repair spent. Also check the insurance path can't be farmed
   (premium vs payout on a car you crash on purpose: payout must stay below repair cost; damage
   while wanted is already excluded).
3. In the garage screen, show the sale price before confirming, and a one-line "how insurance
   works" note next to the insurance buttons (basic: 50 % of the repair bill above $500, $X/day;
   full: 80 % above $250; nothing if the damage happened while wanted).
4. Keep career saves loading (version bump + migration for old saves without the new field).

### Round 12b response (economy, 2026-10-20)
Base: `origin/lead/round12` merged into `ws/economy` (clean).
1. **Resale from the price paid.** New `wp_economy::OwnedCar::purchase_price: Option<Money>`
   (local field; `OwnedCar` lives in `wp_economy`, so no contract change) set by `buy_car`
   (dealer price in the bought condition) and `issue_floor_car` (what was actually paid,
   `min(1 500, cash)`). `sale_price = 0.7 × resale_basis + 0.4 × mods`, scaled by damage, where
   `Economy::resale_basis` is the price paid (old-save cars: cheapest condition, floor price for
   the floor model). Scrap salvage of the body is capped at `0.7 × resale_basis` too (a free
   floor car could otherwise be scrapped for 960). Numbers: Worn 0.6 × base → sells 0.42 × base;
   floor car 1 500 → 1 050 (was 2 100); a floor car given to a broke player for 300 → 210.
2. **Tests:** `crates/wp_economy/tests/resale.rs` — three proptests (192 cases each: every
   roster car × New/Used/Worn × random mod sets × Arcade/Sim): buy → sell / scrap < spent; buy →
   (no / basic / full insurance) → random crash → repair at a legit or crooked shop → sell <
   everything spent, and `insurance_covers < gross`, `player_pays > 0`; floor car for any cash
   0–20 000 sells / scraps ≤ paid. Plus the owner's repro cases, v1-save valuation, and every
   insurance tier paying < any bill (coverage < 1, deductible > 0). Damage while wanted stays
   excluded (`garage_repair::insurance_pays_unless_damage_was_wanted`).
3. **Garage screen:** Sell and Scrap now ask first — "Sell it to the dealer for $X? You paid $Y
   for the car (mods return 40 %)." / "Scrap it for $X (bought for $Y)?" with Sell / Cancel
   (`data-testid="career-confirm"`). The insurance panel has a one-line note built from the data
   (`data-testid="insurance-note"`): "Basic: pays 50 % of the repair bill above $500, $X/night.
   Full: pays 80 % of the repair bill above $250, $Y/night. Nothing for damage taken while
   wanted." `CareerView.cars[].paid` added (= `resale_basis`).
4. **Saves:** `PROFILE_VERSION` 1 → 2 with a real migration step (`step_1_to_2` adds
   `purchase_price: null` to every car); unit test loads a v1 save, values its floor car ≤ 1 500
   and re-saves as v2. The localStorage key (`wp.career.v1`) is unchanged — the JSON version
   inside it carries the migration.

Requests (outside my folders):
- **UI mock (M):** `apps/ui-kit/src/data/economy.ts::salePrice` still ports the old rule
  (`0.7 × basePrice`); the mock Market / Garage screens will show resale above the price paid
  for used cars. Port: body = `0.7 × pricePaid` (and cap scrap body at the same).
- **Testbed e2e (C / lead):** optional extra check in `e2e/run.mjs` career block: after buying
  the beater, `view().cars[0].sale_price < fresh.cash - bought.cash`.


Checks run (2026-10-20): `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`
clean; wasm build + `npm run wasm` OK; `tsc --noEmit` clean in `apps/ui-kit` and
`apps/testbed-web`; `just web-e2e` (`run.mjs` + `race.mjs`) all checks pass. `cargo test
--workspace --no-fail-fast`: everything in `wp_economy` / `wp_wasm::career` passes; red outside
my crates, from sim behaviour on the `lead/round12` base (my diff touches no sim, physics, AI or
data file they read): `simcli` `goldens_match` (5 city scenarios: hashes diverge from tick 240),
`wp_race --test race_picker` `grid_highway_sprint`, `wp_sim --test sidewalk` (1 test),
`wp_wasm` `race_tests::{short_sprint_with_ai_runs_to_results_and_pays_through_the_economy
(player DNF FinishWindow), a_capture_mid_race_is_a_dnf_and_hands_off_to_the_end_of_run_panel
(never captured)}`. For the lead / owners of those crates.
