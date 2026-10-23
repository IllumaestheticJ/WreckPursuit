# Touge port (PR #49) onto `lead/round12`

- **Workstream:** helper for the integration lead: port PR #49 "Touge: four sealed mountain
  passes, boss/challenger battles, side bets and pink slips" (`claude/project-thread-rgdat5`)
- **Branch:** `lead/touge-port` (from `lead/round12` at `db0035d9`)
- **Contract version:** 0.3.1, no contract changes
- **Workstream notes:** `docs/integration/touge.md` (updated for round12)

## What was ported, and what was not

PR #49's branch is three things stacked on `main`:

1. PR #41 free roam (`5f77f20c`, `540d7773`, `05ac8179`, `7b2f2642`): **not ported.** Round12
   already has its own street racers (16 roaming racers, horn challenge, A→B with waypoints,
   bets, Loop 9 rest-stop meets); #41's roaming racers would be a second, competing system.
2. The career branch `claude/project-thread-elq8kz` (`40d0b650`, `8d4a561c`, `1b842ebf` and the
   two merges of it): **not ported.** Another helper ports career progression and city betting
   on `lead/career-port`; bringing it here too would make two copies of
   `wp_economy::progression`.
3. The four touge commits: **ported** by cherry-pick (`2db0b169`, `924a332b`, `3cb2eb55`,
   `253a79cc`), then adapted to round12.

Nothing from `main` beyond what round12 already has came along (no golden conflicts: the PR's
goldens were never merged in).

## Conflict resolutions (one line each)

| File | Resolution |
|---|---|
| `crates/wp_economy/src/lib.rs` | both: round12's `tune` / `visuals` modules and re-exports + `touge` / `WonCar` |
| `crates/wp_wasm/README.md` | both rows: round12's `garage_visit.rs` + the PR's `touge.rs` |
| `crates/wp_wasm/src/lib.rs` (mods) | both: `street` / `street_tests` (round12) + `touge` / `touge_tests` (each test mod keeps its `#[cfg(test)]`) |
| `crates/wp_wasm/src/lib.rs` (exports) | both blocks: round12's street racer + garage visit exports, then the touge exports |
| `crates/wp_wasm/src/race.rs` (struct) | both fields: round12's `resets` + the PR's `touge` |
| `crates/wp_wasm/src/race.rs` (`new`) | round12's `advertised` + `stake_scale`; a touge battle advertises (0, 0) |
| `crates/wp_wasm/src/race.rs` (`post_step`) | round12's `resets.extend` kept; emitted events start with the wall-breach `TotalLoss` events |
| `apps/testbed-web/index.html` | round12's buttons (photo; drift lab / tune buttons stay removed) + the PR's touge button only |
| `apps/testbed-web/src/main.ts` (imports) | both: `GarageVisitView` + `TougeView` |
| `apps/testbed-web/src/main.ts` (panels) | both: garage visit panel + touge view |
| `apps/testbed-web/src/main.ts` (buttons) | round12 (no drift lab buttons) + `sc-touge` handler |
| `apps/testbed-web/src/render/props.ts` (near cars) | round12's allocation-free `nearCars()`, its filter changed from "not a cone" to "is a parked car" (K-rails are neither) |
| `apps/testbed-web/src/render/props.ts` (stats) | round12's `car` line (counts dented cars), same "is a parked car" filter |
| `apps/testbed-web/src/wasm/sim.ts` | both: garage visit calls + touge calls |

## Adaptations to round12 (beyond the conflicts)

- **Side bets use round12's wallet.** Round12 has no rep tiers or bet venue yet (those come
  with the career). Stakes and payouts are cash adjustments on the account ledger
  (`Economy::adjust`, posted as `Adjustment { note: "touge side bet …" }`), the same path as the
  street racers' wagers (`PlayerRun::wager`). No second wallet, no new `EventReason`.
- **Rep and purse:** new `Economy::touge_result` posts the battle's rep as `RaceRep { placement }`
  (1 = won, 2 = lost) and the boss purse as `RacePurse { race: "touge_boss:<pass>" }`: existing
  reasons, so no contract change.
- **Gates:** `touge.toml [gates]` (`wp_race::touge::TougeGates`) replaces
  `require_unlocked(Feature::…)` / `max_bet(BetVenue::Touge)`. The numbers are the career
  branch's `career.toml` tiers (challengers 2000 = Known, bosses and side bets 5000 =
  Respected, pink slips 10000 = Feared, stake ≤ 8000 and ≤ half the cash, none in debt). The
  testbed's "Testbed rep" buttons are 0 / 2000 / 5000 / 10000 to match.
- **Pink slips and the garage:** `take_pink_slip_car` builds round12's `OwnedCar` with
  `tune: Default::default()` and `parts: None` (stock workshop slots, derived from `mods`, which
  is empty for an NPC car). The won car keeps its finishing `DamageSnapshot` and, in a career,
  its full lattice in `CareerProfile::lattices` (as in the PR). A lost car leaves the garage
  through `Txn::remove_car` (clears it as active car), `stats.cars_lost += 1`, posts
  `PinkSlipLost`. `DamageSnapshot::sustained_while_wanted` is round12's
  `sustained_before_capture`.
- **Race catalog:** `wp_race::catalog::race_for_scenario` resolves touge scenario names
  (`race_touge:<pass>:<up|down>:<day|night>:<dry|rain>:<challenger|boss>`) as well as the four
  picker races, so `wp_wasm::race::race_for_scenario` no longer special-cases touge. The
  picker's `races()` list stays at the four races (each with its police profile); touge battles
  have no profile and resolve to police presence 0. `CourseSpec::PoiRoute` gets
  `finish_back_m: 0.0` (the pass's finish POIs already sit 40 m in from the road end).
- **K-rails survive `Sim::set_world`:** round12's `set_world` reseats parked cars on terrain
  and replaces the world's parked list with the kept cars, which dropped the K-rails (stored in
  the parked list) and shifted every later prop index. `World::set_parked_cars` now keeps the
  list's non-car props after the cars (the order the sim inserted them). No-op for every
  existing map (they have only cars there).
- **Dented parked cars:** the testbed's wreck pass skips anything that is not a parked car, so a
  K-rail never gets a car mesh.

## Owner rules

- AI racers never give up: touge battles run through the picker race path (`allow_abandon`
  off, no police); the NPC is a normal race entrant with round12's box-in escape and stuck
  judge. A battle is decided by the wall line or the judge; if the judge ends both cars with
  `DnfReason::Stuck` the player's battle counts as lost (see open questions).
- No real names: the four passes (Hollow Crown Pass, Mistral Ridge, Ashfall Grade, Cedar
  Switchback), eight drivers and their cars (`shiro_turbo`, `hakone_gt`, `kestrel_ts`,
  `lucciola_350`, `sperber_32`, `lindwurm_4t`, all existing round12 cars) are fictional.
- Physics freeze: no `wp_vehicle` change. `wp_damage` gains `DamageModel::declare_total_loss`
  (a game-rule flag, no handling change). K-rails are ordinary static props.
- Determinism: touge uses `wp_math` / f32 only, the side-bet payout is f32 (`round()`), rules
  are pure functions of the setup; scenario seeds come from the setup.
- `wp_contracts` untouched.

## Collisions to expect

- **`lead/career-port`:** `wp_economy/src/lib.rs` (module list and re-exports: both add lines);
  `wp_wasm/src/career.rs` (touge adds `career_account()` and the won-lattice save in
  `end_run`); `wp_wasm/src/run.rs` (touge adds `touge_place` / `touge_settle` /
  `debug_set_rep` next to `enter_race`). Once the career lands, swap the touge gates to it:
  `TougeGates::check` → `require_unlocked(Feature::{TougeRaces, BossRaces, TougeBetting,
  PinkSlips})`, `TougeGates::max_stake` → `max_bet(BetVenue::Touge)`, the two `adjust` calls →
  `place_bet` / `settle_bet`, `Economy::touge_result` → `award_rep` + `award_credits` (pays tier
  rewards), `debug_set_rep` → also `update_progression`, then delete `[gates]`. The PR's
  original code for exactly this is in `2db0b169` / `924a332b`.
- **`lead/ashpeak-port` (PR #50):** both touch `wp_world/src/lib.rs` (new module + `World::`
  constructor next to `vesper`), `wp_wasm/src/scenario.rs` (`city_spec`), `apps/simcli` city
  presets, `tools/city-import/src/bin/gen_city.rs` (presets) and `data/city/` (generated
  greybox / road files). All additive; resolve by keeping both. Ashpeak adds a mountain region;
  touge passes stay separate sealed worlds.

## Gate

See the report sent to the lead (hash, commands, results).

## Open questions

1. Gates: keep the stand-in rep thresholds until career-port lands, or should touge wait for
   the career and use its tiers directly?
2. Both cars stuck (judge's `DnfReason::Stuck`): the battle currently counts as lost for the
   player (stake lost, loss rep). A draw (stake refunded, no rep) may be fairer.
3. The touge passes are not in the testbed's city picker for free drive (as in the PR); if they
   were, round12's street racers would try to roam them.
