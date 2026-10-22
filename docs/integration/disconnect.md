# Disconnect-while-wanted integration notes (B05)

- **Workstream / prompt:** lead helper — backlog B05 "disconnect while wanted = capture after a
  60 s grace" (`docs/design/FEATURE_BACKLOG.md`), needed before the 4-player test
- **Branch:** `lead/disconnect-capture` (from `lead/round12`)
- **Crates / folders touched:** `crates/wp_net` (server), `crates/wp_police` (two hooks),
  `crates/wp_economy` (profile marker), `crates/wp_wasm` (career), `apps/testbed-web` (one
  message)
- **Contract version built against:** 0.3.0 (no contract change; one request below)

## What it does

Quitting is never the best escape:

| Where | Player leaves while… | What happens |
|---|---|---|
| Server (`wp_net`) | wanted (level > 0) or in a pursuit, transport drops | The car stays in the world, AI-held: full brake to a stop, then a light pedal + handbrake (a full pedal at a standstill engages reverse in the automatic gearbox). Heat, wanted level, bounty and pursuit are **frozen** (`wp_police` hold). **Reconnect within 60 s** → same player, car, heat and pursuit, control resumes. **No reconnect** → the pursuit ends `Captured` (capturer = nearest unit assigned to the racer, or none) and the bust is settled on the account like any capture (fine, bail, rep loss, car impounded / seized by mode), then the player and car are removed. |
| Server | wanted, sends `Bye` (clean quit) | Captured and settled immediately (no grace: the player chose to leave). |
| Server | wanted, asks for another role (cop, captain, spectator) | **Refused** (owner decision, 2026-10-20): the player stays a racer in the same car and gets the notice `WANTED_ROLE_SWITCH_REFUSED` ("you are wanted: lose the police or get caught before switching role"). Switching would drop the racer car, i.e. an escape. Only the player's own request is blocked; reassignments the role manager makes by itself are unchanged. |
| Server | not wanted, transport drops | Unchanged: 30 s reconnect window (`ServerConfig::reconnect_window_ticks`), no penalty. |
| Server | any racer busted normally while connected | **New:** the bust is settled on the server account too (it used to be forwarded to clients only), so multiplayer careers reflect busts. |
| Browser career | in a pursuit, tab closed / reloaded | The in-run autosave (every 20 s, after each settlement, and on `pagehide`) carries a `run_in_pursuit` marker. The next load settles it as a bust (fine, impound, the run closes as a captured night) and the garage opens with a "Busted: the game was closed during a pursuit…" message. |
| Browser career | wanted but nobody chasing, tab closed | No bust; the save carries the heat a free end of the run would (`carry_heat(heat, Free)`), so closing the tab does not wipe heat either. |
| Browser career | not wanted | Nothing changes (no marker, no penalty). |

The grace is one constant: `wp_net::server::WANTED_DISCONNECT_GRACE_TICKS = 60 * TICK_HZ`
(the default of `ServerConfig::wanted_grace_ticks`).

Why the police state is frozen during the grace: otherwise a player could drop during the
search phase, let the escape timer run out offline (or let heat decay), and reconnect clean.
Frozen, the outcome is decided only by coming back (resume) or not (capture). A held car can
still be wrecked by contact; a total loss while held is a normal `CapturedTotalLoss` bust.

## Exposes

### `wp_net`
- `server::WANTED_DISCONNECT_GRACE_TICKS` (60 s) and `ServerConfig::wanted_grace_ticks`.
- `ServerStats::{abandoned_captures, busts_settled, role_switches_refused}`; `server::WANTED_ROLE_SWITCH_REFUSED` (the notice text).
- `NetSim` (all with defaults, so other sims compile unchanged):
  `wanted_or_pursued(id) -> bool`, `hold_racer(id, held)`,
  `capture_abandoned(id) -> Option<(WantedLevel, Option<UnitId>)>`,
  `car_spec(car_id) -> Option<wp_economy::CarSpec>`. `WpSim` implements them over `wp_police`
  and its loaded cars.
- Server accounts own no cars yet (everyone drives `racer_car`), so a bust first registers the
  car being driven in the account's garage (no money changes), then calls
  `Economy::handle_capture`. Each ledger event is committed and sent to the owner as a typed
  `SimEvent::Economy`. Economy tables are `Economy::embedded()`, loaded on the first bust.

### `wp_police` (E's crate; additive, no behaviour change for racers that are not held)
- `PoliceSystem::is_wanted_or_pursued(racer)`, `set_racer_held(racer, held)`,
  `is_racer_held(racer)`, `capture_abandoned(racer) -> Option<(WantedLevel, Option<UnitId>)>`.
- Held racers skip `update_racer_rules` (no escape / safehouse / capture rule, no heat change,
  no new pursuit). `capture_abandoned` uses `end_pursuit` (now a thin wrapper over
  `end_pursuit_credited`), so the event order (`PursuitEnded` before the level reset), the
  radio line and unit release are the same as a normal capture. The held set is not in
  `state_hash` (it is only ever non-empty on a server; goldens unaffected).

### `wp_economy` (G's crate; additive)
- `profile::AbandonedRun { car, level, heat, driven_ticks }` and
  `CareerProfile::run_in_pursuit: Option<AbandonedRun>` (`#[serde(default,
  skip_serializing_if = "Option::is_none")]`: old saves load, saves without a pursuit are
  byte-identical to before; profile version stays 1).
- `Economy::settle_abandoned_run(&mut profile, &mut rng) -> Option<Vec<EconomyEvent>>`:
  `handle_capture` at the saved level, then `close_run(Captured)`; clears the marker.

### `wp_wasm` / testbed
- `Career::live_profile` writes the marker while the player is pursued (and the run is not
  already settled); `Career::load_json` settles it; `Career::take_abandoned()` /
  `career_take_abandoned_json()` returns the `RunClose` once.
- `apps/testbed-web/src/main.ts`: after loading the save, `sim.careerTakeAbandoned()`; a bust
  is persisted at once (so a second reload does not repeat it) and the garage opens with the
  message once the testbed is running.

## Tests
- `crates/wp_net/tests/disconnect_capture.rs` (real `WpSim` + `wp_police`, procedural city):
  wanted racer drops → still held and frozen (same heat, level, stationary) at 59 s → at 60 s
  `PursuitEnded { Captured }`, player removed, fine charged, car impounded, `Fine` +
  `CarImpounded` in the ledger; reconnect at 45 s → same player/car/role/heat/level, released,
  no capture after 65 s; `Bye` while wanted → immediate capture; not wanted → 30 s window, no
  capture. Runs in < 1 s.
- `wp_police` unit test `held_racer_is_frozen_and_capture_abandoned_ends_as_captured`.
- `wp_wasm` career tests `closing_the_game_mid_pursuit_is_a_bust_on_the_next_load` and
  `closing_the_game_when_not_pursued_is_free`.
- `crates/wp_net/tests/vesper_e2e.rs` (4 clients, racer 1 drops for 3 s): fails on
  `lead/round12` **with or without this branch** (`p0: Escaped` vs `Captured`, the issue F
  fixes on `ws/net` in `1acbd20`). With `1acbd20` cherry-picked on top of this branch it
  passes (52.8 s, debug profile). Its assertions are not changed here.

## Contract change requests
| # | Change | Why | Who is affected | Status |
|---|---|---|---|---|
| DC-1 | `PursuitOutcome::Abandoned` (or `abandoned: bool` on `PoliceEvent::PursuitEnded`) | An abandoned car is reported as plain `Captured` today (closest existing variant). HUDs, the radio ("suspect abandoned the vehicle"), cop XP (`award_cop` should arguably not pay a full capture for a car nobody drove) and stats cannot tell them apart. Economy settles it exactly like `Captured`. | 06 police, 08 economy, 07 net, UI | proposed |

## Changes outside my crates
All of this branch is outside a lead-owned crate; listed for the owners:
- `crates/wp_police/src/lib.rs`, `pursuit.rs` (E): the hold set and the two methods above;
  `end_pursuit` split into `end_pursuit` + `end_pursuit_credited` + `pursuing_credit`.
- `crates/wp_economy/src/profile.rs`, `lib.rs` (G): `AbandonedRun`, the profile field,
  `settle_abandoned_run`.
- `crates/wp_net/src/server.rs`, `netsim.rs` (F): grace, hold inputs, settlement, `NetSim`
  hooks. The disconnected-car input changed from a constant full brake to brake-then-handbrake
  (the old one could engage reverse at a standstill and creep away over a long grace).
- `crates/wp_wasm/src/career.rs` (G/C), `apps/testbed-web/src/main.ts`, `src/wasm/sim.ts` (C).

## Known gaps
- Session violations are not tracked server-side, so server fines are the level fine only
  (`CaptureContext::session_violations` = 0); the browser career's in-run busts are
  unaffected (the session tracks them), but an abandoned-run bust uses zero violations too.
- A normal multiplayer bust settles the ledger (car impounded on the account) but the server
  does not yet act on it physically: the racer keeps driving the same car. What a bust does to
  a live player (respawn, spectate, garage) is F's / the design's call.
- A role switch the role manager makes by itself (not requested by the player) still drops a
  wanted racer's car without a capture; today no such path moves a seated racer, but a new one
  should check `wanted_racer_seat` first.
- `kick` never counts as a capture (an admin action, not the player's choice).
- The browser marker is only as fresh as the last save: `pagehide` normally fires on a tab
  close or reload; if the browser is killed without it, the last autosave (≤ 20 s old) decides.
- A save exported mid-pursuit and imported later is settled as a bust on import (intended).

## Gate (2026-10-20, cloud container, `CARGO_INCREMENTAL=0`, dev/test debug off)
| Check | Result |
|---|---|
| `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings` | clean |
| `cargo build --target wasm32-unknown-unknown -p wp_wasm`, testbed `tsc --noEmit` | clean |
| `just web-e2e` | all checks passed (incl. career bust → reload → release → repair) |
| `cargo test --workspace --no-fail-fast` | all pass except two groups that fail identically on `lead/round12` without this branch: simcli `goldens_match` / `all_scenarios_pass…` (11 scenarios; first-mismatch hashes byte-identical with and without this branch, so it moves no golden), and `vesper_e2e` (F's `1acbd20` fixes it; passes with that cherry-picked on top of this branch) |
