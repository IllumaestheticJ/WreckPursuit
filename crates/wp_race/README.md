# wp_race — race types, judging, event scheduler, crews & territory

The content layer of the open world: which races exist, when, with what rules, how results are
judged, and the crew/territory meta that gives night racing stakes beyond cash. `wp_net` owns
the lobby transport; this crate owns the rules. Workstream 15 (machine L), branch `ws/race`.

Pure Rust, deterministic, wasm32-clean (`cargo build --target wasm32-unknown-unknown -p wp_race`).
Depends on `wp_contracts`, `wp_math`, and read-only on `wp_world` (routes, POIs, zones),
`wp_economy` (payout types), `wp_opponent` (AI entrants) and `wp_police` (report input path). Data lives in `data/events/` (see its README).

## Design

### Race kinds

| Kind | Course | Win condition | Special rules |
|---|---|---|---|
| `Sprint` | fixed corridor, ordered gates | first across the last gate | leaving the route is not a DQ; gates must be passed in order |
| `Circuit { laps }` | closed loop, gate 0 = start/finish | first to complete `laps` | best lap tracked |
| `Outrun { lead_m }` | open world (course = start line only) | lead by `lead_m` measured **along the leader's own trail** (breadcrumbs every 2 m; straight line if the chaser left the trail), else leader at the time limit | lead changes when the chaser is `overtake_margin_m` ahead along the leader's heading; opponent DNF/DQ → survivor wins |
| `Checkpoint` | open route, ordered gates, any path | first through every gate; time limit | no corridor |
| `Cannonball` | cross-city, start + finish, any path | first to finish | no corridor; scheduler asks for heavy traffic |
| `Delivery { cargo_kg, cargo_value }` | corridor | first to finish | contact impulse above a threshold damages cargo; cost = value × damage; destroyed cargo = DNF; the cargo mass is on the car (`cargo::load_cargo`) |
| `Drag` | 402 m straight, side-by-side grid | lower elapsed time | jump start (moved > 0.3 m before green) = DQ; reaction time, upshift-in-window score, trap speed |
| `TimeAttack` | single-file grid | best **own** elapsed time | each racer's clock starts at their own start-line crossing |

Every kind is a rule object (`RulesTable::for_kind` → `RaceRules`): start-grid layout, min/max
racers, time limit, finish window after the winner, plus shared tolerances. A course can
override the grid layout (`Course::grid_layout`): `catalog::plan_course` lines the field up
single file in the route's lane when a slot of the kind's two-wide grid (or its first 30 m)
would overlap a parking lane, so nobody starts in the parked cars on a narrow street.

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
  server), time limit, finish window, retire/disconnect, cargo destroyed, lost outrun, fled
  from the police (AI entrants, reported by `entrants`).
- **DQ:** jump start, implausible movement (faster than 110 m/s between samples — teleport/anti-cheat),
  marshal. Leaving the route is never a DQ (owner call, 2026-10-20): the gates still have to be
  passed in order, so a shortcut that skips one gains nothing. `relocate()` lets the server
  reset a car without tripping the teleport check.
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

**Paying the stake through `wp_economy` (R5).** The economy keeps the money and has five
race types and no legality. `bridge::economy_race_type` maps kinds onto them (Sprint,
Checkpoint, Cannonball, Delivery → Sprint; Circuit, TimeAttack → Circuit; Outrun → Touge;
Drag → Drag) and `bridge::economy_race` returns the economy's `RaceSetup` plus
`purse_scale` / `entry_scale` (advertised stake ÷ the economy's Arcade numbers), so a scaled
settlement pays exactly what the lobby advertised; the economy's placement shares, wanted
multiplier, clean bonus and Simulation premium still apply on top. The scale is ≈ 1.0 at the
shared feel target (Compact 9 km night street sprint), and carries legality and the
per-kind premium elsewhere. Applying the scale inside `settle_race` is G's side (see
`docs/integration/race.md`, R5).

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

### AI entrants (`entrants`)

`AiEntrants` puts machine N's `wp_opponent::Field` into a judged race: one field on the race's
`wp_world` route, extended `AI_RUN_OFF_M` (60 m) past a point-to-point finish so the AI drives
through the gate instead of parking at the end of its corridor. Cars are held on the handbrake
until the judge's green tick (not the brake: auto-reverse turns a held brake at a standstill
into reverse). The judge stays the authority: the AI's own civilian-contact DQ is off, and
`wp_opponent` outcomes the judge cannot see become judge DNFs (`Abandoned` → `Fled`,
`Retired` → `Retired`), once each.

### Police (`police`)

`wp_police` has no race API before contract v0.3.0, so race signals use its existing input:
`SimEvent::TrafficViolation`. `police::report_events` turns `PatrolAttracted` (a hot meet)
into one `Reckless` report per racer within 400 m of the venue (heat, and an unwitnessed one
files a civilian report that dispatches a patrol). `police::deliver` feeds them to a
`PoliceSystem` with a zero-length step until `wp_sim` can queue external events for its own
police step (request to the lead in `docs/integration/race.md`).

### Delivery cargo (`cargo`)

`load_cargo(car, &CargoLoad::in_boot(&car.def, kg))` returns a `LoadedCar` carrying the cargo:
mass added, front weight fraction and CG height moved to the combined centre of mass, principal
inertias grown by the cargo box plus both bodies' parallel-axis terms. `wp_vehicle` builds the
chassis body and static wheel loads from those fields, so spawning the Delivery entrant with
this car makes the cargo felt (400 kg in a sedan: 22 % less distance in 6 s from rest, 30 %
longer stop from 25 m/s; `tests/cargo_physics.rs`).

### Honk-to-start spots (`spots`)

`race_spots(world)` puts a meet spot (radius `SPOT_RADIUS_M` = 12 m, ±4 m height window) at the
pole position of every catalog race that runs on the world's city (`catalog::races_on`; Port
Vesper's races run on `vesper_v2` and on the combined `ashpeak` map, `RaceCity::hosts`). A race
whose course does not plan gets no spot. `honk_spot(spots, pos, speed)` is the spot a honk starts:
the one the car stands in, slower than `HONK_MAX_SPEED_MPS` (3 m/s). The horn edge belongs to the
browser session's one challenge system (round 12 street racers: a racer within honking range
takes a honk first; `wp_wasm::session`, `wp_wasm::race_spots`); the race's money is the picker's
existing stake (advertised buy-in / purse).
Ashpeak County's races (`RaceCity::Ashpeak`) run on the combined `ashpeak` map only
(`tests/race_spots.rs`).

### The regulars (`roster`, `racers.toml`)

Twelve named regulars make up the city's picker-race scene (not the roaming street racers
of `wp_wasm::street`, who have their own levels and names). Each has a stable id, name and
nickname, a car (`data/cars`, analog names), a home turf, a course **specialty** (hill,
highway, downtown, mixed), a **tier** (the licence tier at which they start turning up at
scheduled meets: rookies at once, Nina "Static" Castellanos only for elite players), a
`wp_opponent::DriverProfile` and a starting **form rating** (Elo-like, 1000 = mid-pack).
Every AI driver in the picker is one of them (`catalog::AiDriverDef::id`; a test checks the
picker and the file agree on name, car, style and profile, and that every regular races
somewhere).

`RosterState` is the save-game side: starts, wins, podiums, DNFs, peak rating and the last
five results per racer, the player included (`PLAYER_KEY`). `record_race` updates them from a
judged result with a multi-player Elo step (every pair of racers is one game; K = 32 spread
over the field; a DNF loses to every finisher, two DNFs draw). Ratings are integers and
`pow10` is built from `+ − × ÷` only, so every target agrees. `field_for_event` fills a
scheduled meet with regulars (tier-gated, weighted towards locals and towards ratings near the
player's), and `resolve_offscreen` draws a Plackett–Luce finishing order for races nobody
watched, so the regulars' records keep moving while the player is elsewhere.

### Betting (`betting`, `betting.toml`)

A `Market` prices one race from its entrants' ratings (form + a specialty bonus of 60 on a
matching course + a car adjustment for the player's car against the field's median class
index):

| Market | Probability | Margin |
|---|---|---|
| Win | Plackett–Luce `sᵢ / Σs`, `s = 10^(rating/400)` | 15 % |
| Podium (fields of 4+) | exact Plackett–Luce over every ordered top three | 12 % |
| Head-to-head | `sᵢ / (sᵢ + sⱼ)` | 6 % |

Offered odds `1 / (p · (1 + margin))`, rounded down to 0.05, clamped to 1.05–30.0, stored as
integer thousandths so payouts are exact. A `Book` takes tickets until the green light, then
settles them from the result (win / top three / finished ahead; a finisher beats any DNF; a
head-to-head where neither finishes, or a ticket on a racer who never started, is void) or
refunds everything (`void_all`). **A racer in the race may only back themselves** (win,
podium, or a head-to-head with themselves as the pick), so nobody can profit from throwing a
race. Money moves only through `BetWallet`; the career economy decides whether and how much
the player may bet (`wp_economy::Economy::max_bet / place_bet / settle_bet`, rep tier caps in
`data/economy/career.toml`). A 2 000-race simulation checks the book keeps its edge.

### Picker races (`catalog`)

Owner, 2026-10-24: the picker (`catalog::races()`, and so every meet spot and race card) offers
only the Loop 9 races.

| Id | Where | Kind | Terrain | Hour | Length | Field |
|---|---|---|---|---|---|---|
| `loop9_circuit` (Loop 9 night) | Port Vesper | Circuit ×1 | highway | 23:00 | 10.1 km | Vic, Mara, Dez, Juno, Tito |
| `loop9_day` (Loop 9 day) | Port Vesper | Circuit ×1 (illegal daytime) | highway | 14:00 | 10.1 km | Vic, Mara, Dez, Juno, Tito |
| `loop9_ashpeak_highway` | Ashpeak map | Circuit ×1 | highway | 00:30 | 17.9 km | Vic, Mara, Dez, Juno, Tito |

The highway run is one lap of Loop 9's east side, Route 7 (Talon Tunnel, Silver Gorge), the
Crosstown Expressway, Route 12 over the Grizzly Grade and back onto Loop 9 at Lantern Way:
highway lanes all the way (the polygon's points sit on the lanes the ramps feed, because the
planner does not change lanes).

**Retired races** (`catalog::retired_races()`): `hollins_sprint`, `lantern_checkpoint`,
`grid_highway_sprint`, the Ashpeak County races (`crowfoot_ascent`, `crowfoot_descent`,
`harnell_dirt_run`, `ashpeak_downtown_checkpoint`, `route7_tunnel_sprint`,
`grizzly_grade_sprint`, `waterfront_drag`, `ashpeak_metro_circuit`) and PR #47's city races
(`harbor_dash`, `tunnel_run`, `hollins_descent`, `offramp_sprint`, `grid_arterial_sprint`).
They are offered nowhere in the game, but `catalog::race(id)` and `race_for_scenario` still
find them, so their `pick_<id>.ron` scenarios, goldens and `tests/race_picker.rs` runs stay as
regression tests (and the browser's `?race=<id>` debug link still starts one).

`catalog::betting_market(def, roster, state, rules, car_adjust)` prices any of them.
### Touge battles (`touge`, `data/events/touge.toml`)

One-on-one sprints on the four `wp_world::touge` passes. `TougeSetup` = pass × uphill/downhill ×
day (14:00) / night (23:30) × dry / rain × challenger / boss; its scenario name
`race_touge:<map>:<up|down>:<day|night>:<dry|rain>:<challenger|boss>` round-trips, and
`race_def` / `plan_course` build a sanctioned single-file sprint (NPC ahead, 12 m gap, no
traffic, no police). Every pass has a fixed challenger and boss (`ROSTER`).
- **Rules:** `Boundary::check` flags a car through the wall line (a K-rail stretch) or fallen
  well below the road; `TougeBattle::observe` decides the battle at once for the other car
  (the car through is totaled). Otherwise the first across the line wins.
- **Gates (career API, `wp_economy::progression`):** racing a challenger needs
  `Feature::TougeRaces`, a boss `Feature::BossRaces`; side bets need `Feature::TougeBetting`
  (stake up to `max_bet(BetVenue::Touge)`), pink slips `Feature::PinkSlips`. `wager_menu` lists
  each line with the locked reason; `place` refuses a locked or oversized wager.
- **Settlement (`settle`):** side bet via `settle_bet` (challenger 2.0×, boss 2.5×), boss purse
  (12 000), rep via `award_rep` (`boss:<map>` / `touge:<map>`; −100 extra for going through the
  wall), and pink slips: the winner takes the loser's car **exactly as it finished**
  (`Economy::take_pink_slip_car` with the NPC's damage snapshot; `lose_pink_slip_car`).

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
- `tests/race_police.rs` — `apps/simcli/scenarios/race/city_sprint_police.ron` (traffic + police
  on): four `wp_opponent` AI entrants race a judged sprint, the hot venue's `PatrolAttracted`
  is called in through `police`, and a pursuit starts; a control run without the report has no
  pursuit; the run is deterministic.
- `tests/race_vesper.rs` — `apps/simcli/scenarios/race/vesper_hollins_sprint.ron`: four AI
  entrants race the Port Vesper v2 hill route up Hollins Drive (~1 km, hairpins, summit crest)
  to the Knob Observatory Lot; at least three finish, nobody is wrecked; deterministic.
- `tests/race_picker.rs` — every race of the browser's picker (`catalog`), raced to the finish
  from `apps/simcli/scenarios/race/pick_<id>.ron` with AI in every seat and the browser's glue
  (`live::LiveRace`, meet report, stake heat, the tow of busted AI cars 6 s after capture via
  `StepOut::impound`); expectation ranges per race (winner time,
  finishers, no wrecks, patrol + heat for the illegal daytime run); deterministic.
- `src/roster.rs`, `src/betting.rs` unit tests — roster data, Elo updates (upsets, DNFs, form,
  save round-trip), deterministic `pow10`, meet fields (tiers, locals), off-screen results;
  market consistency (win sums to 1, podium to 3, implied > 1), odds rounding, the
  back-yourself rule, stake limits, settlement and voids, long-run house edge.
- `tests/cargo_physics.rs` — Delivery cargo slows the car in the real `wp_sim` loop.
- `tests/determinism_lint.rs` — runs `tools/lint-determinism` over `wp_race/src`.

## Performance (native, release, cloud container, 2026-10-19)

| What | Measured | Note |
|---|---|---|
| Judge, 12 cars, every physics tick | 0.195 µs/car/tick (≈ 0.0008 ms/car per 60 Hz frame) | includes the scripted driver |
| Calendar generation | 2.0 µs/day (24 venues) | |
| Picker race glue (`live::LiveRace`: AI field + judge + standings), per physics tick | 4.7–17.1 µs (0.02–0.07 ms per 60 Hz frame), 4–6 racers | `WP_RACE_TIME=1 cargo test --release -p wp_race --test race_picker -- --nocapture` |

No budget is set for race logic in `CLAUDE.md`; this is three orders of magnitude below the
police budget (0.5 ms/frame).

## Known gaps

- Outrun's trail gap falls back to the straight-line distance when the chaser is more than
  25 m off the leader's trail (took another street), which can under-count a lead.
- Progress for DNF ranking projects onto the centreline; on open-route kinds a racer far off the
  suggested line ranks by gates passed first, so this only matters between racers with equal
  gates.
- `Checkpoint` gates must be taken in order (no any-order variant yet).
- Drag shift scoring needs the car's redline (`RacerEntry::redline_rpm`; 0 = skipped).
- Delivery cargo is applied at spawn (`cargo::load_cargo`); destroyed cargo stays in the car's
  mass (no unload mid-race without a `wp_vehicle` payload hook). Lateral cargo offset only
  changes inertia, not the CG (the contract has no lateral CG).
- Race → police goes through a cloned `PoliceSystem` (`police::deliver` + `Sim::set_police`)
  until `wp_sim` queues external events into its police step.
- Purses: the economy pays its own table (R5, agreed with G: it prices kinds and legality
  itself). The two tables agree within a third at every kind × legality × phase the calendar
  schedules (tested); off-calendar combinations (e.g. a street race at noon) can differ ~2×.
- Signals are crate-local until the contract carries a typed race event (request R1).
- Crews/territory are in-memory; persistence belongs to the server's account store.
- Picker races (`catalog` + `live`) run in the browser session (`wp_wasm::race`); see
  `docs/integration/race.md` → Step 2 for the browser-side gaps (reset to the grid slot, no
  closures for street races, simcli replay of a race replay drifts).
- Betting in the browser covers bets on yourself in picker races. Spectator betting on
  NPC-only races (`Book::new(market, None)` + `resolve_offscreen`) and filling scheduled meets
  with `field_for_event` have their APIs and tests but no caller yet (the server / meet UI).
- Roster records live in the browser (localStorage `wp.roster.v1`); a server keeps them per
  world, not per player (not wired yet).
- AI entrants (`entrants`) are wired in tests and the browser; `wp_net`'s server must spawn them at
  `ScheduledEvent::spawn_s` and call `AiEntrants::tick` / `report_to` (request to F).
  simcli cannot drive them in a standalone scenario run yet (requests to I and N).
