# Career port integration notes (PR #46 + PR #47 onto lead/round12)

- **Workstream:** helper for the integration lead: port two `main`-based PRs onto `lead/round12`
- **Branch:** `lead/career-port` (from `lead/round12` at `db0035d9`; never pushed to round12 or main)
- **PRs:** #46 `claude/project-thread-elq8kz` (career progression), then #47
  `claude/project-thread-7fsvk6` (named regulars, betting on picker races, five new city races)
- **Contract version built against:** 0.3.1 (no contract edits)

## Result in one paragraph

One wallet, one betting system. The wallet is the session's `PlayerRun` account (the career
account during a career run), as both round12 and #47 already used it. Every bet now goes
through `wp_economy`'s `place_bet` / `settle_bet` (#46), with a venue per kind of bet:
`Street` for round12's roaming street racers (honk to challenge), `City` for #47's bets on
yourself in a picker race (`Touge` stays for later). Round12's street-racer flow is
unchanged for the player: same levels, stake ranges, even money, no tier lock. The career
tiers, unlocks, dealer gates and tier bonuses from #46 sit on top. The picker now lists nine
races: round12's four, then #47's five, all driven by #47's twelve named regulars, and every
race has its own police profile in `data/police/races.toml`.

## Conflicts and how each was resolved

PR #46 (9 conflicts; three of them come from PR #41, which #46's branch is based on):

| File | Resolution |
|---|---|
| `wp_economy/src/data.rs` | both: round12's `parts` table + #46's `progression` (`career.toml`) in `DataSources`, `Economy`, parse, validate |
| `wp_economy/src/error.rs` | both: round12's `PartRefused` / `UnknownPart` + #46's `Locked` |
| `wp_economy/src/ledger.rs` | both `PlayerAccount` fields: `career` (#46) + `inventory` / `next_part_uid` (round12), all serde-default |
| `wp_economy/src/lib.rs` | both module docs, modules and re-exports |
| `wp_opponent/src/lib.rs` | both doc bullets (`evader` + `roam`) |
| `wp_wasm/src/session.rs` step 1c | round12's street-racer block, then #41's roam block (two separate scenarios, never both at once) |
| `wp_wasm/src/career_tests.rs` | round12's tune + workshop tests, then #46's tier test |
| `ui-kit/src/garage/CareerGarage.tsx` | header lists both the tune (round12) and the Career tab (#46) |
| `testbed-web/src/main.ts` | round12's career mods + drift-lab links kept; #41's `free_roam` restart and `?roam=N` added; #41's `?drift` → `startDriftLab()` dropped (round12 retired those labs) |

PR #47 (5 conflicts left after #46):

| File | Resolution |
|---|---|
| `wp_race/src/catalog.rs` | both: round12's `point_along` + #47's `betting_market` / `roster_key_of`; the five new `PoiRoute`s gain round12's `finish_back_m: 0.0` |
| `wp_wasm/src/lib.rs` | both: round12's street-racer / garage-visit exports, then #47's `race_bet`, `race_bet_status`, `roster_*` |
| `wp_wasm/src/race.rs` | `RaceRun` keeps round12's `resets` and gains #47's `book` / `bets` / `rating_changes`; the buy-in uses round12's `charge_entry` (stake scaling), the book is built just before it |
| `testbed-web/src/main.ts` | both: round12's garage-visit panel + #47's roster save/load |
| `testbed-web/src/ui/race.tsx` | `RaceCard` keeps round12's `police_style` / `police_summary` and takes #47's richer `drivers` |

Goldens: no golden conflicted. Only the five new race goldens (`race_harbor_dash`,
`race_tunnel_run`, `race_hollins_descent`, `race_offramp_sprint`, `race_grid_arterial_sprint`)
differed and were re-blessed: #47 blessed them on main's map at contract 0.3.0, round12's map
puts their grids elsewhere (regenerated `pick_*.ron`) and the header says 0.3.1. Every other
golden hash is unchanged.

## Design choices

1. **One wallet = `PlayerRun`'s account.** `PlayerRun::{max_bet, bet_lock, place_bet,
   settle_bet}` take a `BetVenue`; #47's `*_city_bet` helpers and round12's `wager` are gone.
   Ledger notes are #46's `bet:stake:<venue>:<label>` / `bet:payout:<venue>:<label>`.
2. **Round12's street racers keep their flow (new venue `Street`).** `Feature::StreetBetting`
   opens at `Unknown` (`career.toml`), so a new player can still challenge every level.
   `max_bet(Street)` is the whole cash: no tier cap and no 50 % cash fraction, because the
   racer's level range (`street::LEVELS`) is the limit, as before. Debt still blocks. Even
   money is unchanged (`settle_bet(ticket, 2 × stake)`). The only visible change: the ledger
   line reads `bet:stake:street:vs <name> (<level>)` instead of "street race stake vs …".
3. **#47's picker-race bets stay as built (venue `City`).** They open at the Street tier, are
   capped by the tier's `max_bet` and 50 % of cash, use the odds book, and close at the green
   light. The browser docs now say "picker-race betting", not "street-race betting", so the two
   don't get mixed up.
4. **Street-racer wins award rep by level** (round 2, lead's answer): `street::Level::win_rep`
   Street 40 / Pro 70 / Elite 110 / Legend 160 (a picker-race win is ~100–200), posted with
   `PlayerRun::award_rep` (`career:rep:street:vs <name> (<level>)`). A loss costs nothing extra.
   The result JSON and the testbed banner show it (`+N rep`).
5. **Tier bonuses:** paid at career run end, abandoned-run settlement and next night (#46),
   and also at once by every `award_rep` (street-racer wins, touge results) and by the
   testbed's `debug_set_rep` (lead's touge rule). A promotion earned that way mid-run is
   already booked when the run closes, so the end-of-run panel's `progress` does not show it
   again (the ledger does).
6. **The regulars race the picker races only.** Round12's roaming street racers keep their own
   name pool and levels. The name overlap (street racers "Mara" / "Juno" vs regulars Mara
   Lindqvist / Juno Park) was already there in round12; left as is.
7. **Catalog order:** round12's four races first (menu order unchanged), then #47's five. Course
   length hints that changed on round12's map: `tunnel_run` 1323 → **1267 m**,
   `offramp_sprint` 1623 → **2304 m**. The other three match.
8. **Police profiles for the five new races** (`data/police/races.toml`; `wp_police`'s builtin
   test now lists all nine). They are light: tactics that fit each course plus stakeouts, and
   no scripted roadblocks or spikes on the course (no box-ins, row 154). Harbor "on-ramp radar"
   (no tools, PIT/rams, 1 stakeout); Tunnel "interceptors" (no tools, no helicopter, 1
   stakeout); Descent "bottom of the hill" (no rams or PIT on the hairpins, junction blocks, 1
   stakeout near the bottom); Off-ramp "patrolled streets" (normal presence, nothing
   scripted); Grid arterial "block patrol" (1 stakeout).
   The off-ramp table started at 1.5× presence with two stakeouts, and the police captured the
   whole field on most seeds of round12's longer course. With a profile equal to the default
   police it passes on its seed (8) and on 5 and 11, but not on 3 (see the gaps below).
9. **PR #41 (main's free roam) is dormant, as on round12's main-sync** (round 2): only its own
   `free_roam` scenario via `?roam=N`, no testbed button, never in a session with the street
   racers (`docs/integration/main-sync.md`). The round12 merge took main-sync's version of
   every roam overlap.
10. **Naming overlap to watch:** the street-racer levels "Street" / "Legend" share words with
    the rep tiers "Street Racer" / "Street Legend". They're not linked; a UI pass may want to
    rename one of them.
11. **Owner rules:** no `wp_vehicle` / handling change; no `wp_contracts` edit; no wall-clock
    time or unseeded randomness added (odds and tiers are integer / f32 table math); AI
    never-give-up untouched (street races keep `allow_abandon: false`; picker races use
    round12's entrants). The regulars, races and cars use analog names only.

## Round 2 (lead answers; current lead/round12 merged; touge on the career)

Merged `lead/round12` at `5080fa80` (main-sync with contract 0.4.1 and the Arcade / Simulation /
Extreme modes, the touge port, police heat levels, PR #52 tailing challenges). Conflicts (10):

| File | Resolution |
|---|---|
| `wp_economy/src/data.rs` | `progression` + round12's three-mode table (Extreme) |
| `wp_race/README.md`, `wp_race/src/lib.rs` | both (regulars / betting / picker table + touge); picker table lengths updated to round12's map |
| `wp_wasm/src/career_tests.rs` | #46's tier test + round12's engine-stand test |
| `wp_wasm/src/run.rs` | round12's `touge_place` / `touge_settle` / `debug_set_rep` (then switched to the career, below) |
| `wp_wasm/src/race.rs` | `RaceRun` keeps the betting book and gains `touge`; a touge battle returns before the book is built and settles through its own path (no regulars' form, no picker bets) |
| `wp_wasm/src/session.rs` | round12's step numbering (1d roam); the duplicate `debug_run_mut` dropped (round12's public one stays) |
| `wp_wasm/src/lib.rs` | both: touge exports + picker-bet / roster exports, both doc rows |
| testbed `wasm/sim.ts`, `main.ts` | both: race + touge types; roster save + touge view |

Touge onto the career system (the lead's list; PR #49's own code, `claude/project-thread-rgdat5`,
ported back with round12's later fixes kept):

- `TougeGates` / `Gate` and `touge.toml [gates]` deleted. `Opponent::feature()` →
  `Feature::{TougeRaces, BossRaces}`; side bets `Feature::TougeBetting`, pink slips
  `Feature::PinkSlips` (`Economy::require_unlocked`); stake cap `max_bet(BetVenue::Touge)`.
- `wager_menu(econ, acct, table, setup)` (tier id from `rep_tier`), `place(econ, acct, setup,
  wager, car)` takes the stake with `place_bet` (`WagerTicket::bet: Option<BetTicket>`), and
  `settle` pays it with `settle_bet`, the boss purse with `award_credits`
  (`career:credits:touge_purse:…`) and rep with `award_rep` (`career:rep:boss|touge:<pass>`; it
  pays tier rewards; rep still floors at 0 in every transaction).
- `Economy::touge_result` removed (no callers left; one money path).
- `PlayerRun::debug_set_rep` also runs `update_progression`.
- Touge NPCs get `AiDriverDef::id`s `touge_<first name>` (not city regulars) and their
  `RaceDef` `terrain: Hill`.
- round12's touge tests kept and moved to the career API (`rep_for(Feature)` from
  `career.toml`; the side-bet test checks a `bet_stake` ledger note).

Round 2 gate (same env): `cargo fmt` clean; clippy `-D warnings` clean; `cargo test
--workspace` 1576 passed, 0 failed, 42 ignored; wasm32 `wp_wasm` OK; testbed `npm run wasm` +
`tsc` OK, ui-kit `tsc` OK; `e2e/street.mjs` 105/105 (the first run failed the two touch-horn
checks: the 40 × 250 ms "place behind and honk" loop timed out under load; the re-run passed
all), `e2e/race.mjs` 26/26, `e2e/touge.mjs` 15/15. Goldens: the five new race goldens are
header-only blessed (contract 0.3.1 → 0.4.1, no tick hash moved). **Three goldens fail on
`lead/round12` itself** (checked in a clean worktree of `5080fa80`, same hashes): 
`jam_pursuit_vesper` (first mismatch tick 12000), `police_tunnel_escape_founders_cut` and
`…_control` (tick 16320; expectation `police_event_before(kind:12, t_s:95)` never). Not
blessed here: they belong to round12 (the police heat-level change, row 156, is the likely
cause).

## Round 3: races and touge from the career garage (playtest row 167)

Owner: "I need to be able to reach the touge and race features in the career menu, instead of
having to go back to the main menu to get to it." Base: `origin/main` `aa30f9ea` (the branch was
restarted there; rounds 1–2 are in main).

- **Garage start row** (`ui-kit` `CareerGarage`): Night run, Free drive (already there), and now
  **Street races** and **Touge battles** (`CareerApi.races?()` / `touge?()`, optional so other
  hosts hide them; disabled when the career car can't go out, like the others). The touge
  button's tooltip names the tier touge needs while it is locked.
- **Street races** open the race picker as a career run: cards priced for the career car and
  mode, bet limits from the career (`progress.max_bet_city`; locked below the Street tier with
  the reason). "Race" starts `career_start_run(race_scenario_json(…))` (new wasm export), so
  buy-in, purse, bets and rep go through the career. **Fix:** `Session::begin_career` now
  re-charges a picker race's buy-in on the career account (`RaceRun::reenter`); before, it went
  on the session's throwaway account (test
  `a_picker_race_from_the_garage_charges_the_career_account`).
- **Touge battles** open the touge picker on the career's account (`tougeUsesCareer()` is true
  for a garage-opened picker): locked battles and wagers show the rep tier they need
  (`require_unlocked`, unchanged rules).
- **Back to the garage:** closing a garage-opened picker reopens the garage; a career race's
  results show **Back to garage** (instead of Race again / Races… / Free drive), and a decided
  career touge battle shows it on its panel. Both end the run through `openGarage()` (settled
  on the career, one night passes). "Restart" on the end-of-run panel of a career race / battle
  also goes to the garage. Main-menu entries are unchanged (quick play stays outside the
  career).
- **Map** (agreed with the ashpeak-port helper): career races and garage night runs / free
  drives use the main menu's remembered map (`loadMenuMap(MENU_MAPS)`, key `wp.menu.map.v1`);
  the career race picker lists `racesForMap(…, citySel.value)` like the main one, and the
  menu's `play()` now applies the map for the garage choice too (only touge stays off-map).
- **e2e:** race.mjs (+5: garage → Street races → career race with the career car, buy-in off the
  career → results → Back to garage, settled; closing the picker returns), touge.mjs (+3:
  garage → Touge battles reads the career's tier, locked; closing returns). run.mjs and tune.mjs
  were broken on main by round 1's Dealer lock-badge column (`td:last-child` became the badge
  cell): their "buy worn" selector now targets `td:nth-last-child(2)`.

## Round 4: street bets reach the career (playtest row 169)

Owner: "street race bets don't go to career". Base `origin/main` `431232c5`.

- **Cause:** not the wasm side. A street race inside a career run already stakes and pays on
  the career run's account, and `finish_run` writes it back (new test
  `street_bets_settle_on_the_career`: one lost, one won race in a garage-started free drive →
  both stakes, the even-money payout and the level's rep are in the career's ledger). The
  testbed's **main menu** started Night run / Rainy night / Free drive as quick play
  ("Quick play runs outside the career") even with the career box ticked, so those bets went on
  the quick-play account and never reached the career. (The developer panel's buttons already
  followed the box.)
- **Fix:** with the career box ticked (the default), a career, and a career car that can go out
  (`can_start`), the main menu's Night run / Rainy night / Free drive start **career runs**
  (`startCareerRun`, which gained `rain` and `city` arguments; restarts keep the rainy night).
  Otherwise they stay quick play.
- **Career off / no usable career car = quick play:** betting still works on practice cash
  that does not count for the career, and the street UI says so: the bet slip's stake line
  carries a tag, "quick play: not your career" (or "career cash"; a tooltip explains), and the
  result banner says "quick-play cash" / "career cash".
  `street_status_json` gained `career` (the session is a career run).
- **e2e (street.mjs, +4):** main menu → Free drive with the career box on is a career run and
  the bet slip says career cash; with it off, quick play and the slip says so.

## Contract change requests

| # | Change | Why | Who is affected | Status |
|---|---|---|---|---|
| 1 | #46's request #1 (typed `EconomyReason` career / bet variants), with `venue` now also `"street"` | the client feed should not parse `Adjustment` notes | 07 net, testbed UI | proposed (carried over) |

## Gate (lead/career-port head)

All green, with `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`:
`cargo fmt` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test
--workspace` 1516 passed, 0 failed, 42 ignored (187 suites); `cargo build --target
wasm32-unknown-unknown -p wp_wasm` OK; testbed `npm run wasm && npx tsc --noEmit -p .` OK
(ui-kit `tsc` OK too); `e2e/street.mjs` 105/105 PASS; `e2e/race.mjs` 26/26 PASS (#47's bet
slip included); `simcli goldens`: only the five new race goldens changed (blessed, see above).
Also: `race_picker` in release, 12/12 (the five new races race to the finish with AI in every
seat).

## Known gaps

- `offramp_sprint` on seed 3: the police capture the whole field before anyone finishes
  (`race_picker` asserts a finisher). The race's own seed (8) and seeds 5 and 11 pass. Round12's
  police on the 2.3 km signal-heavy course are the cause, not the profile; the lead may want
  a shorter course or a lower presence.
- `RaceRun::reenter` (the session adopts the previous account) keeps the betting book built on
  the first account. That's harmless, since bets can only be placed after the session exists,
  but it isn't rebuilt.
- The testbed has no street-racer UI for the career tier (not needed: `Street` is open from the
  start).
