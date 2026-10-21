# PROMPT 22 — First-time player flow (machine S; `ws/onboarding`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`, `docs/STATUS.md`,
`docs/design/GDD.md` (§6 assists, §9 onboarding, §10 failure states), `docs/design/CITY.md`,
`docs/design/FEATURE_BACKLOG.md` (B03, B04, B07, B08, B10) and the integration docs of the
systems you compose: `economy.md` (G), `race.md` (L), `opponent.md` (N), `police.md` (E),
`world.md` (D), `ui.md` (M), `testbed.md` (C), `meta.md` (O).

## Goal
A brand-new player goes from first frame to their own garage in about 15 minutes, in the real
city, without a tutorial wall, through five beats in this order:

1. **Intro crash** — a borrowed car, a real physics crash, the damage model on full show.
2. **Junkyard car** — wake up with 4 000, buy the junkyard beater.
3. **First race** — a free, short meet a careful beater can place in.
4. **First police chase** — the first real violation draws a pursuit; escape or get caught.
5. **Garage** — the repair bill for everything that happened, legit vs crooked shop explained.

Every beat runs on the normal systems (vehicle, damage, world, traffic, police, race, economy).
Nothing is canned: the flow only chooses *where* things start, *what* is placed in the world
and *which hint* to show, then reacts to `SimEvent`s. Every outcome a beat allows (avoiding the
crash, losing the race, being captured) leads on to the next beat. The flow is deterministic,
skippable per beat, and resumable.

## Ownership
You own:
- `crates/wp_onboarding` (new crate: the flow state machine, beat definitions, hint triggers;
  no renderer dependency, must build for `wasm32-unknown-unknown`)
- `data/onboarding/*.toml` (beat tuning, placements and locations per city, hint copy)
- `crates/wp_wasm/src/onboarding.rs` (a self-contained module plus `pub mod onboarding;`. C owns
  the rest of `wp_wasm`)
- `apps/testbed-web/src/onboarding/` plus an `onboarding.html` page (C owns the rest of the
  testbed; reuse M's screens from `apps/ui-kit` and do not fork them)
- `apps/simcli/scenarios/onboarding/`
- `docs/integration/onboarding.md` (from `docs/integration/_TEMPLATE.md`)
- `docs/media/onboarding/`

Never edit `wp_contracts`. File contract changes under "Contract change requests" in your
integration doc. If you must touch another crate to compile or wire something, keep the change
minimal, list it under "Changes outside my crates" and tell the owning machine.

## What exists (verified 2026-10-20) and what does not
- **Economy (G).**
  - `data/economy/market.toml`: `starting_cash = 4000`; floor car `junkyard_beater` at 1 500,
    Worn.
  - `Economy::open_account` gives no car. `Economy::issue_floor_car` *is* the junkyard offer:
    it is only allowed with no usable car, costs `min(1500, cash)` and emits `FloorCarIssued`.
  - Races: `race_setup` / `enter_race` / `settle_race`. A `RaceType::Meet` has free entry and a
    250 minimum purse.
  - Repairs: `quote_repair` / `repair`, with `ShopKind::{Legit, Crooked}` and
    `RepairOption::{RepairAll, RepairSelected, DriveDamaged, Scrap}`.
  - Police outcomes: `handle_capture`, `handle_escape`, impound.
  - Glue: `EconomySession` (per run) and `crates/wp_wasm/src/run.rs` `PlayerRun`.
  - G is building a garage and career that persist between runs (STATUS step 2): use it rather
    than inventing your own save.
- **Cars.** Intro car `rampart_50` ("Rampart 5.0", Muscle, RWD). Player car `junkyard_beater`.
  Parked patrol car for beat 4: `sentinel_cruiser`.
- **Race (L) and opponents (N).**
  - The pieces exist: `wp_race::bridge::plan_city_race`, `RaceJudge`, `entrants::AiEntrants`,
    the economy bridge, and `wp_opponent::Field`.
  - They are **not wired into `wp_sim`/`wp_wasm`** yet. L is building race-in-city with a race
    menu (STATUS step 2). Coordinate with L: use their wiring if it lands first; otherwise
    compose the race in your module and hand the glue to L.
- **Police (E).**
  - Pursuits start only from *witnessed* violations: line of sight within `witness_range_m`, or
    a camera.
  - Escape timers come from `data/police/heat.toml`. Safehouse dwell is 10 s.
  - `PoliceEvent::{PursuitStarted, WantedLevelChanged, PursuitEnded{outcome}}`.
  - Reference scenarios: `golden/night_pursuit_escape.ron` and
    `crates/wp_police/scenarios/{escape_break_contact,safehouse,capture_stop}.ron`.
- **World (D).**
  - `vesper_v2` (the testbed default) has race-start POIs (e.g. "Lantern Run"), safehouses
    ("Mercer Wash-n-Fold Lot", …), garages ("Meridian Motorworks" legit; "Sal's Paint & Panel",
    "Gantry Garage" crooked) and precincts. Look POIs up by filtering
    `world.network().pois` by name.
  - **There is no junkyard POI.** Put its position in `data/onboarding/vesper_v2.toml` and file
    a contract request for `PoiKind::Junkyard` plus a map entry with D.
  - Garage POIs have no legit/crooked flag yet (B03): keep the mapping in your data until D adds
    it.
  - Calloway Tunnel and R10 Mercer Meet exist only in `handmade_v0` / CITY.md, which has no road
    graph. Do not depend on them.
- **UI (M).**
  - `apps/ui-kit` has `Hud`, `ChaseHud`, `DamageSilhouette`, `Minimap`, `Market`, `Garage`,
    `RepairBill`, `Lobby`, `Results`, `Impound` and `Settings`, plus the live adapter
    `apps/ui-kit/src/live/`.
  - The testbed already mounts `chaseHud`, `repairBill` and the run-end panel.
- **Assist presets do not line up.**
  - The GDD has Rookie / Street / Sim.
  - The UI kit has `off | sim | standard | full | custom` (default `full`).
  - wasm `assist_preset` takes an input source.
  - B10 (M + A) owns the fix. Ship an explicit mapping in your data and switch to B10's presets
    when they land.
- **Events.** There is no event bus: the flow drains `Sim::drain_events()` after each step and
  reads `SimEvent::{Contact, TotalLoss, TrafficViolation, Police(..), Race(..), Economy(..)}`.

## The flow
Model it as a pure state machine in `wp_onboarding`:
`Flow::new(cfg, seed)`, `Flow::step(tick, &[SimEvent], &FlowView) -> Vec<FlowAction>`.
- `FlowView` is a read-only snapshot: player pose and speed, wanted level, account cash, active
  car health, nearest POIs.
- `FlowAction`s are what the host executes: spawn/place a car, place a parked patrol, set the
  route, open a screen, show a hint, call an economy function, set the assist preset, fade, save
  progress.
- The host (your wasm module and the simcli runner) owns the `Sim`, `Economy` and race
  composition. The state machine never touches them directly, so it can be unit-tested with
  synthetic events.

Each beat has an entry placement, an exit condition, every allowed alternative outcome, a time
budget and a skip.

1. **Intro crash (≈2 min, night, borrowed `rampart_50`).**
   - The player drives a short, staged stretch of `vesper_v2`: a 300–600 m street ending in a
     signalled junction, chosen in data.
   - A physical AI traffic car runs the red into the junction. Time it from the player's
     arrival estimate (distance / speed) so that a player doing the limit meets it at
     35–60 km/h.
   - If the player brakes, swerves or stops, a second car follows. If they still avoid
     everything, the beat ends at the junction with a "near miss" line and the damage lesson
     moves to the race.
   - On impact: the camera holds, the damage silhouette and the panels that came off are
     highlighted, and one line explains it ("Every dent changes how a car drives — and what it
     costs."). The car is not theirs, so there is no bill.
   - Exit: 4 s after the first contact over the impulse threshold in data, or at the junction.
   - Never block on a total loss. `TotalLoss` is a perfectly good ending.
2. **Junkyard car (≈1 min).**
   - Fade to the junkyard at the data position.
   - `open_account` (4 000), then the junkyard beater offer through `issue_floor_car`.
   - The Market screen shows that one car with its price, condition and a one-line "Worn: it
     will need repairs". Buy is the only button; it is not skippable on a fresh account.
   - Spawn the car at the yard gate. The player drives out to a marker 100–200 m away. This is
     the brake-and-steer check: first use of throttle, brake and a turn, with hints only if the
     player does nothing for 5 s.
   - Exit: the marker is reached.
3. **First race (≈5 min).**
   - A `RaceType::Meet` (free entry) on the shortest suitable `vesper_v2` start (≤ 4 km sprint;
     pick in data, "Lantern Run" first) with 3–4 AI entrants.
   - Entrants: N's beginner driver profile in Beater/Compact-class cars, tuned so a careful
     clean beater can place top 3 against them. No rubber-banding (GDD §6.2): tune the field,
     not the physics.
   - Lobby screen with one line about the stakes, then countdown, race, and the Results/payout
     screen explaining purse, clean bonus and rep.
   - A DNF (total loss, leaving the corridor) is allowed: show the §10 cost (no purse, −20 rep)
     in one line and continue.
   - Settlement goes through `settle_race`, so the money is real.
4. **First police chase (≈5 min).**
   - After the race the flow sets a route to the garage and places one parked patrol on it,
     facing the route, inside `witness_range_m` of the road.
   - A pursuit starts **only** through E's normal rules, from the player's first witnessed
     violation (speeding past the unit is the usual one).
   - The radar and heat-meter hint fires on `PoliceEvent::WantedLevelChanged`. On
     `PursuitStarted` the HUD highlights the two nearest safehouses, and one line explains line
     of sight.
   - If the player drives legally past the patrol, a second parked unit is placed further on.
     If they are still clean on arrival, the chase beat is marked "avoided" and the garage
     beat says so ("Clean run — the cops never noticed you").
   - All outcomes continue: `Escaped`/`Safehouse` → bounty via `handle_escape`; `Captured` →
     the real §10 capture costs and impound, the Impound screen explaining retrieval, then the
     car goes to the garage.
   - Keep E's tables. If the chase is too hard for a beater, raise it with E rather than
     special-casing the police.
5. **Garage (≈3 min).**
   - Go to the nearest garage by driving (or by tow after a capture or total loss).
   - Pick the legit shop by default. Show the crooked alternative in one line ("Crooked shops
     don't report wanted cars — and charge for it"), and make it the suggestion when the
     player is wanted.
   - `RepairBill` comes from the real accumulated damage (race + chase) via `quote_repair`,
     with RepairAll / RepairSelected / DriveDamaged. All are valid; debt is possible and is
     explained.
   - Then the Garage screen with the mod tree teaser.
   - Exit: the garage screen is closed. Mark onboarding complete in the player's profile.
     The flow ends there, with the player in their garage: no follow-up card or role offer
     (the normal game takes over from the garage).

**Cross-cutting rules:**
- **Assists.** The default is the GDD "Street" preset (through your mapping).
- **Rookie suggestion.** Count crashes over the impulse threshold in the first 5 minutes. On
  the third, *suggest* Rookie with one prompt. Never switch silently, and never ask twice.
- **Hints.** At most one line on screen at a time, each shown at most once, all triggered by
  events (not timers, except "idle 5 s"). Copy lives in `data/onboarding/hints.toml`.
- **Skips.** Every beat can be skipped from the pause menu. Skipping applies the beat's
  defaults:
  - intro: no effect;
  - junkyard: car bought;
  - race: not entered, so no purse and no rep change;
  - chase: avoided;
  - garage: quote shown, nothing paid.
  A "Skip intro" choice on the first screen jumps to beat 2 for returning players.
- **Resume.** Progress (current beat plus the facts later beats need) is saved after every
  beat exit through G's persistent profile/career. Until that lands, use the browser account
  plus a localStorage beat marker (per-viewer convenience only). Reloading resumes at the
  start of the current beat.
- **Determinism.** No wall clock and no unseeded randomness. Every placement, timing and AI
  choice comes from data plus the flow seed. Ticks are integers.

## Work
1. **`wp_onboarding` crate:** the state machine, beat structs, `FlowAction`, data loaders with
   validation (positions on the network, POI names exist), and unit tests per beat:
   - every exit;
   - every alternative outcome;
   - skips;
   - resume from each beat;
   - the Rookie-suggestion rule (exactly once, never silent).
2. **Data:** `data/onboarding/{flow.toml, hints.toml, vesper_v2.toml}`.
   - `vesper_v2.toml`: intro street and junction, junkyard position and gate marker, race start
     and entrants, patrol placements, garage choice per shop kind.
   - Validate them in a test against the loaded city.
3. **Host glue:**
   - `crates/wp_wasm/src/onboarding.rs`: drives a `Sim` + `Economy` + race composition from
     `FlowAction`s, with the smallest accessors the page needs. List them in your doc for C.
   - A simcli runner for `apps/simcli/scenarios/onboarding/*.ron` (coordinate the format with
     I).
4. **Browser:** `onboarding.html` + `src/onboarding/`. Mount M's screens (Market, Lobby,
   Results, ChaseHud, Impound, RepairBill, Garage) through the live adapter, and add the hint
   line, the fades and the pause-menu skips.
5. **Scenarios and tests (range expectations, not exact numbers, except determinism):**
   - One scenario per beat with a scripted driver reaching its exit. Include the alternative
     outcomes: crash avoided; race lost/DNF; chase escaped, safehouse, captured and avoided.
   - A full flow start-to-garage with a bot driver. Assert the account at the end is
     consistent with the ledger (cash = 4 000 − 1 500 + purse − fines − repairs).
   - The full flow's hash is identical twice and native == wasm.
   - A broke-player path: capture plus a repair larger than cash still reaches the garage exit
     (§10 "never a dead end").
6. **Playtest pass:**
   - Time each beat with the bot and by hand in the browser; budget ≈15 min total.
   - Record results in `docs/PLAYTEST_LOG.md` rows.
   - Take Playwright screenshots of each beat to `docs/media/onboarding/`.
   - Add a `just onboarding` recipe if the justfile owner agrees (list it otherwise).
7. **Docs:** a crate `README.md` (what it does, how to run its tests, known gaps), and
   `docs/integration/onboarding.md`:
   - Exposes / Needs: L race wiring; D junkyard POI and B03 shop flag; G persistence; M screens
     and B10 presets; E tuning feedback; N beginner profile.
   - Contract requests: `PoiKind::Junkyard`; a profile `onboarding_state` field via G/O.
   - Changes outside your crates, perf, known gaps.
   - Update B08 in `docs/design/FEATURE_BACKLOG.md` when done.

## Gate before every push
`cargo fmt`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`cargo build --target wasm32-unknown-unknown -p wp_wasm`, the determinism lint, `npx tsc --noEmit`
in `apps/testbed-web`, `just web-e2e` if it touches the testbed. Use
`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0` to save disk.
Commit often (`wp_onboarding: …`, `data/onboarding: …`, `testbed/onboarding: …`) and push
`ws/onboarding`.
