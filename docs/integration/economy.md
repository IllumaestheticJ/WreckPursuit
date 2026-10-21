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
| `record_damage(&mut acct, &DamageReport)` | end of session: health, total_loss, handling, estimate, lattice blob, `wanted_during_session` (informational; insurance only cares about captures), `towed` (towing is a penalty) |
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
