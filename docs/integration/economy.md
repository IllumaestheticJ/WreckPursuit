# Economy integration notes

- **Workstream / prompt:** 08 — economy, progression, mods
- **Branch:** `ws/economy`
- **Crates / folders owned:** `crates/wp_economy`, `data/economy/`, `data/mods/`
- **Contract version built against:** 0.2.0

Design write-up, tuning levers and measured numbers: `crates/wp_economy/README.md`.

## Exposes

Everything is a method on `wp_economy::Economy` (immutable data bundle). Mutating calls take
`&mut PlayerAccount` and return `Result<(T, Vec<EconomyEvent>), EconomyError>`; a refused call
never changes the account. Persist with `AccountStore::commit(&account, &events)`.

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
| `quote_repair(&acct, car, &RepairOption, &ShopContext, mode) -> RepairQuote` | `RepairAll`, `RepairSelected(items)`, `DriveDamaged`, `Scrap`; district + crooked/legit labour; insurance |
| `repair(...) -> RepairOrder` | the damage system must apply the order to the stored lattice and send a fresh `DamageReport` (`towed: false`) |
| `repair::reference_estimate(&PriceTable, scale, &[(RepairItem, severity)])` | formula stand-in for `wp_damage` |

### Payouts
| Call | Notes |
|---|---|
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
| 06 police | ~~Capture/escape payload~~ covered by `PursuitEnded` + `WantedLevelChanged` (pursuit wiring above). **New for v0.3.0:** `PursuitEnded` should name the capturing unit and assisting units (`capturer: Option<UnitId>, assists: Vec<UnitId>`) so `award_cop` can pay player cops; and the event order (ended before the level reset) should stay as it is or carry `level` in the event. Consume `InformantTip` / `ShopReported` / `BountyCleared`. Feed the risk-budget overspend for `CopAction::RiskBudgetOverspent`. Read `ToolLoadout` (radar/scanner/fake plates) | `CaptureContext` built by hand in tests |
| 03 damage | (a) ~~unscaled `parts_cost` / `labour_hours`~~ done in v0.2.0; (b) an API to serialize / restore lattice state (`Vec<u8>`) and to apply a `RepairOrder` (restore listed items / full reset); (c) `gen-lattice` extension: create detachable groups for body-kit `DetachableRequest`s | `reference_estimate`; lattice blob stays empty |
| 02 vehicle | ~~Tire files~~ landed; `market.tire_grip` is now each file's `lateral.d` / street's (a test requires an entry per `data/tires/` file). Optionally `wp_vehicle::metrics` to replace the estimator; consume `ToolLoadout.run_flat`, `reinforced_cooling`, `nitrous` | estimator + grip table |
| 09 cars | A Beater car file `junkyard_beater` (base price 3 000) for the economy floor (`market.toml [floor_car]`); keep `base_price`s inside the `market.toml` class bands | floor car is economy-only data |
| 10 harness | CI line `cargo test -p wp_economy --features native` so the SQLite store is tested in CI | run locally |

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| 1 | Promote the economy event payload: add `SimEvent::Economy(EconomyEvent)` (or a typed `EconomyEventKind` enum) with the shape of `wp_economy::ledger::EconomyEvent` / `EventReason`, replacing the opaque `{ kind, payload }` | the client feed / net layer need typed events (amounts, reasons, car ids); today they get JSON bytes | 07 net, 04 testbed UI, 12 | accepted → v0.3.0; final variant list below |
| 2 | `RepairEstimate` promotion refinements: document that `RepairLine::parts_cost` and `labour_hours` are **unscaled** and only `total` includes `repair_cost_scale`; add `PriceTable::labour_hours: BTreeMap<RepairItem, f32>` (per-item hours) with `labour_hours_per_item` as the fallback | billing re-prices labour per district / shop and must not double-apply the mode scale; a roof costs more labour than a mirror | 03 damage, 08 | **done in v0.2.0**; `price_table` fills `labour_hours` + `repair_parts_fraction` from `prices.toml` |
| 3 | `RepairEstimate::health: f32` and `total_loss: bool` (or a `DamageSummary` returned next to the estimate) | the economy needs both at session end; today `DamageReport` carries them separately | 03, 08, 12 | deferred → v0.3.0 (`DamageSummary`) |
| 4 | `DamageSim::{save_state() -> Vec<u8>, restore_state(&[u8]), apply_repair(&[RepairItem], full: bool)}` | `DriveDamaged` must carry the lattice across sessions; partial repairs must restore only the paid items | 03, 08, 12 | accepted → v0.3.0 (03 implements) |
| 5 | Move `WantedLevel` from `wp_police` into `wp_contracts` | economy, net and police all key tables by it; today `wp_economy` depends on `wp_police` just for this enum | 06, 07, 08 | **done in v0.2.0**; `wp_economy` now uses `wp_contracts::WantedLevel` and no longer depends on `wp_police` |
| 6 | Optional: `DistrictId` type in contracts (prompt 05 ids) | repair labour is keyed by district name strings (`prices.toml [district_labour]`) | 05, 08 | deferred → v0.3.0 |

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
None. (The previous stub's public API in `wp_economy` changed — `PlayerAccount::garage` is now
`Vec<OwnedCar>`, `AccountStore` is `load -> Result<Option<_>>` / `commit` / `events_since`,
`repair_bill` is replaced by `quote_repair`/`repair` — no other crate used it.)
Dependencies: `wp_contracts`, `wp_math`, `serde_json`, `toml`, `thiserror` (all wasm-clean;
`wp_police` dropped in v0.2.0); `rusqlite` (bundled) optional behind `native`; dev-only
`proptest` and `wp_damage` (a test keeps `repair::reference_line` equal to `wp_damage`'s pricing
constants).
Versions for `rusqlite`/`proptest` are pinned in the crate's `Cargo.toml`, not the workspace
table (lead may move them).

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `preview`, 7 mods | cloud container, release | 167 µs | — | not per-frame |
| `preview`, welded cage | cloud container, release | 248 µs | — | not per-frame |
| `enter_race` + `settle_race` | cloud container, release | 0.4 µs | — | not per-frame |
| `handle_capture` | cloud container, release | 0.2 µs | — | not per-frame |
| 20-night career (60 races) | cloud container, release | 168 µs | — | — |

## Known gaps
- Lattice blobs and partial repairs wait on `DamageSim::{save_state, restore_state, apply_repair}`
  (v0.3.0). Real estimates come from `wp_damage` once the sim calls `record_damage`;
  `reference_estimate` now prices with exactly `wp_damage`'s formula for tests and the career.
- The floor car file `junkyard_beater` does not exist yet (09); no Beater-class car is in the roster.
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
