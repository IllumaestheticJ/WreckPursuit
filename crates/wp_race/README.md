# wp_race — race types, judging, event scheduler, crews & territory

The content layer of the open world: which races exist, when, with what rules, how results are
judged, and the crew/territory meta that gives night racing stakes beyond cash. `wp_net` owns
the lobby transport; this crate owns the rules. Workstream 15 (machine L), branch `ws/race`.

Pure Rust, deterministic, wasm32-clean (`cargo build --target wasm32-unknown-unknown -p wp_race`).
Depends on `wp_contracts`, `wp_math`, and read-only on `wp_world` (routes, POIs, zones) and
`wp_economy` (payout types). Data lives in `data/events/` (see its README).

## Design

### Race kinds

| Kind | Course | Win condition | Special rules |
|---|---|---|---|
| `Sprint` | fixed corridor, ordered gates | first across the last gate | corridor DQ |
| `Circuit { laps }` | closed loop, gate 0 = start/finish | first to complete `laps` | best lap tracked |
| `Outrun { lead_m }` | open world (course = start line only) | lead by `lead_m` (straight-line gap), else leader at the time limit | lead changes when the chaser is `overtake_margin_m` ahead along the leader's heading; opponent DNF/DQ → survivor wins |
| `Checkpoint` | open route, ordered gates, any path | first through every gate; time limit | no corridor |
| `Cannonball` | cross-city, start + finish, any path | first to finish | no corridor; scheduler asks for heavy traffic |
| `Delivery { cargo_kg, cargo_value }` | corridor | first to finish | contact impulse above a threshold damages cargo; cost = value × damage; destroyed cargo = DNF |
| `Drag` | 402 m straight, side-by-side grid | lower elapsed time | jump start (moved > 0.3 m before green) = DQ; reaction time, upshift-in-window score, trap speed |
| `TimeAttack` | single-file grid | best **own** elapsed time | each racer's clock starts at their own start-line crossing |

Every kind is a rule object (`RulesTable::for_kind` → `RaceRules`): start-grid layout, min/max
racers, time limit, finish window after the winner, plus shared tolerances.

### Judging (server side)

`RaceJudge` is a pure state machine fed by the server each tick (or each net tick):

```text
Lobby ──start(tick)──► Countdown (3 s) ──► Running ──nobody running──► Finished
```

- **Checkpoint validation.** A gate counts when the segment between two consecutive samples
  crosses its line *forwards*, within `width/2 + base + per_mps × speed` (1 m + 0.02 s × speed).
  Speed scaling keeps a 60 Hz feed as accurate as 240 Hz (tested). Several gates in one coarse
  sample are taken in order.
- **Finish timing** uses the crossing fraction between samples, in integer *sub-ticks*
  (1/1000 tick), so finishing order is exact and identical on every target.
- **DNF:** total loss (`SimEvent::TotalLoss`), police capture (`dnf(.., Captured)` from the
  server), time limit, finish window, retire/disconnect, cargo destroyed, lost outrun.
- **DQ:** outside the corridor (`corridor_half_width + 2 m`) for more than 3 s (shortcuts),
  jump start, implausible movement (faster than 110 m/s between samples — teleport/anti-cheat),
  marshal. `relocate()` lets the server reset a car without tripping the teleport check.
- **Clean race:** finished, no contact above 2 500 N·s (racers, walls, police), no civilian hit
  above 400 N·s, no `Collision` violation. Speeding alone does not break it.
- **Ranking** is a total order (`RacerResult::rank_key`): finishers by elapsed time → running/DNF
  by gates passed, distance, time survived → DQ by the same → grid slot → entity id. No tie is
  left unresolved (property-tested).

### Stakes (`stakes.toml`)

```text
risk  = kind.risk + phase.risk + legality.risk
purse = kind.base_purse × bracket.purse × risk_curve(risk)
entry = purse × kind.entry_frac × phase.entry × legality.entry
heat  = kind.heat + phase.heat + legality.heat     (0 for sanctioned events)
```

`risk_curve` is strictly increasing, so for a fixed kind and bracket the purse is monotone in
risk by construction (property-tested). Feel: a D-class night street sprint ≈ 2 800 (matches
`wp_economy`'s own target), an illegal daytime sprint ≈ 2.5× that with police presence `Max`,
sanctioned day circuits pay least and cost no heat.

### Scheduler (`schedule.toml`)

`Scheduler::generate_day(day, venues, meet_heat)` builds the day's calendar from
`(seed, day)` only — every server and client agrees:

- **Sanctioned** day events (1–2/day, 09–16 h) on closed-circuit venues: `TrafficMode::Suppressed`
  (→ `wp_world::ActiveRoute { closed: true }`, cones), marshals (`Course::marshal_spots`),
  police presence `None`.
- **Night meets** (5–8/night, 21–04 h) at race-start POIs, with spawn time, lobby window and start.
- **Illegal daytime** races: 8 % of days, 11–17 h, `police_presence_override: Max`, licence tier
  2 and 1 500 rep required.
- **Meet heat:** each race adds 1.0 heat to its venue, −0.25/h; at 2.5 a patrol is attracted
  (`RaceSignal::PatrolAttracted`) and the scheduler picks that venue 5× less often.
- Same-venue events keep a 40-minute gap. Times are integer seconds; night events after
  midnight keep `start_s ≥ 86 400` on the calendar day they belong to.

### Crews and territory (`crews.toml`)

- Crews: create (unique name), join (max 8), leave (leader hands over to the lowest account,
  last one out dissolves the crew and gets the bank), shared rep (25 % of members' race rep),
  bank cut (default 10 %, leader may set 0–30 %), leader withdrawals.
- Crew challenges: leader issues (district, kind, bracket, bank stake, optional pink slip) →
  defender accepts (stakes escrowed) or declines → resolved from race results: the crew of the
  best finisher wins the pot; pink slip names the winner's best car and the loser's last-placed
  car for `wp_economy::settle_pink_slip` (racers still consent there). No finisher → refund.
- Territory = `wp_world` zones. Wins in a district earn points (street 10, illegal daytime 25,
  sanctioned 4, +3 clean, podium 5/2); challenge wins earn 20 and steal 25 % of the loser's
  points. Most points ≥ 40 controls the district (tie keeps the incumbent). Control gives
  `heat_decay_multiplier` 1.15 and the district safehouse. Points decay 35 % per in-game week.

### Progression (`progression.toml`)

Class brackets D/C/B/A/S/X from `wp_economy`'s `class_index` (boundaries checked against
`data/economy/market.toml`), licence tiers Rookie/Street/Pro/Elite/Legend at 0/5/15/40/100 clean
races, `EntryRequirements { brackets, min_licence, min_rep, crews }` checked by
`ProgressionTable::check_entry`.

## Tests

```sh
cargo test -p wp_race                       # unit + property + integration tests
cargo run --release -p wp_race --example judge_bench
```

- `src/tests.rs` — every kind through deterministic scripted traces (`trace::simulate`),
  DNF/DQ ordering, clean detection, sample-rate independence, stakes, scheduler, crews,
  territory, progression; property tests: scoring is a total consistent order (no unresolved
  ties, finishers ≺ DNF ≺ DQ), stakes monotone with risk.
- `tests/simcli_races.rs` — judges real physics runs of `apps/simcli/scenarios/race/*.ron`
  (drag coupe vs sedan with reaction/shift/trap; corridor-cutting DQ; a city sprint on the
  procedural city's arterial, route planned by `wp_world`), checks judging is deterministic,
  checks race closures through `World::set_active_routes` (closed lanes, cones, no civilians on
  the route), and runs sprint + circuit courses planned on the procedural city.
- `tests/determinism_lint.rs` — runs `tools/lint-determinism` over `wp_race/src`.

## Performance (native, release, cloud container, 2026-10-19)

| What | Measured | Note |
|---|---|---|
| Judge, 12 cars, every physics tick | 0.195 µs/car/tick (≈ 0.0008 ms/car per 60 Hz frame) | includes the scripted driver |
| Calendar generation | 2.0 µs/day (24 venues) | |

No budget is set for race logic in `CLAUDE.md`; this is three orders of magnitude below the
police budget (0.5 ms/frame).

## Known gaps

- Outrun measures the straight-line gap, not the gap along the road.
- Progress for DNF ranking projects onto the centreline; on open-route kinds a racer far off the
  suggested line ranks by gates passed first, so this only matters between racers with equal
  gates.
- `Checkpoint` gates must be taken in order (no any-order variant yet).
- Drag shift scoring needs the car's redline (`RacerEntry::redline_rpm`; 0 = skipped).
- Delivery cargo mass is not applied to the car yet (needs a `wp_vehicle` hook, see
  `docs/integration/race.md`).
- Signals are crate-local until the contract carries a typed race event (request R1).
- Crews/territory are in-memory; persistence belongs to the server's account store.
- No AI entrants yet (prompt 17); the scheduler only says when cars spawn.
