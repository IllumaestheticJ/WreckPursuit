# Police radio v2 integration notes (owner playtest row 79)

- **Workstream:** lead helper — "cops don't talk with each other" (row 79), on top of the row-6
  pacing fix (`lead/playtest-radio`).
- **Branch:** `lead/police-radio-v2` (from `origin/lead/round12`, not main)
- **Crates / folders touched:** `crates/wp_police` (`radio.rs`, new `chatter.rs`, small hooks in
  `lib.rs` / `dispatch.rs` / `pursuit.rs` / `tools.rs` / `config.rs`), `data/police/radio.toml`,
  `crates/wp_world` (road names on `World`), `crates/wp_audio` (one parity test),
  `apps/testbed-web` (radio strip, scanner voices, settings), `apps/ui-kit` (one setting).
- **Contract version built against:** 0.3.0 (no contract change: `PoliceEvent::Radio { unit, text }`
  is enough).

## What changed

### 1. Callouts driven by what happens (`wp_police::chatter`)
A new observer module. Once per police step it reads the step's police events
(`PitAttempt`, `UnitDisabled`, `SpikeHit`), the sim's contacts (PIT contacts of physical units)
and, on think ticks, diffs racer and unit state. Every line starts with a call sign and is placed
on the map.

| Callout | Trigger | Tier |
|---|---|---|
| "Unit 6, lost visual. Last seen eastbound on Harbor Boulevard at Lantern Way." | pursuit → searching for 1.5 s | Critical |
| "Unit 3, I've got eyes on him again, northbound on Ridge Road." | searching → pursuit after an aired lost call (an unaired lost call is withdrawn instead) | Critical |
| "Unit 2, requesting backup, …" (dispatch's existing "copy, sending unit n of m" follows) | new reinforcement request | Tactical, prio 2 |
| "Unit 9 in pursuit of racer #7 …, ETA 40 seconds." / "… moving to intercept …, ETA …" | far units (> 150 m) joining | Tactical |
| "Unit 4, I'm on his bumper, …" | pursuer ≤ 14 m behind a moving racer for 1.5 s | Tactical, 2.5 s life |
| "Unit 4, going for the PIT." → "he's spun! Suspect is spun out …" / "negative on the PIT, he got away …" | PIT tactic; verdict 2.5 s after the contact (racer < 5 m/s or turned ≥ 60°) | Tactical |
| "Unit 5, boxing him in, …" | two or more pursuers in box-in slots | Tactical |
| "Unit 10, spikes hit! He's losing tires …" | racer over a strip | Tactical, prio 4 |
| "Unit 1, my unit's wrecked on X. I'm out of the chase." | `UnitDisabled` of a unit on a racer (Routine "Out of service" otherwise) | Critical |
| "Unit 3, he's gone into the Hollins Tunnel, westbound!" | pursued racer enters a covered road | Tactical, prio 4 |
| "Unit 5, copy, searching on X, ETA …" | a unit sent to intercept while the racer is lost | Tactical |
| "Dispatch, all units: racer #7 is now level 4, southbound on X. 3 units on him, air unit overhead." | level rises during a pursuit (any code path); a newer summary replaces a queued one | Critical |
| "Dispatch: Unit 2 has racer #7 in custody on X. Code 4, all units." / "we've lost racer #7, last seen …" / "pursuit … terminated" | pursuit end | Critical |
| Roadblocks / spikes: "Units 7 and 8 setting up a roadblock on Harbor Boulevard." | (was "segment 120") | Critical |

**Places** (`PoliceSystem::place_phrase(pos, vel)`): heading (when moving) + "on <street>" +
" at <cross street>" within 30 m of a junction; "in the <tunnel>" inside a covered road; the
district ("downtown", "in the suburbs", "in the industrial district") on unnamed roads (the
procedural grid). Street names: `World::edge_names()` (new, from Port Vesper's generator report,
per `RoadEdgeId`), set by `PoliceSystem::for_world` through `chatter::attach_world`; tests and
other hosts can call `set_road_names` / `set_tunnels`.

### 2. Pacing (`wp_police::radio`, `data/police/radio.toml`)
- **One transmission at a time.** A spoken line holds the channel for its estimated length
  (`words / 2.6 + 0.7 s`, the audio scanner's numbers; a test keeps them equal) plus
  `min_gap_s = 2.5` of silence. Before, every critical line was spoken in the step it was raised,
  so a pursuit start could put 2–3 lines on top of each other.
- **Priority queue:** tier, then the cue's `prio`, then age. Lines expire (`stale_s` 6 s,
  `stale_critical_s` 15 s, or the cue's own life: bumper/PIT 2.5 s) and can be superseded
  (`RadioCue::supersedes(slot)`; `RadioDirector::withdraw(slot)`).
- **Emergency traffic** (`RadioCue::breaks_in`): "Unit N, I've been hit!" (assault on an
  officer) and captain priority orders cut a normal transmission and go out at once (flagged
  `cut_in` in the log; the audio scanner chops the line it is playing for the same reason). A
  break-in never cuts another break-in. This keeps the existing "assault line in the same
  moment" behaviour (`wp_sim/tests/police_assault.rs`, `wp_net/tests/city_v03.rs`).
- **More chatter as heat rises, quiet on patrol:** `global_gap_s = [8, 6.5, 5, 4, 3.5]` and
  `unit_gap_s = [30, 22, 16, 12, 10]` by the highest wanted level (L1..L5). **L1 = the row-6
  values (8 s / 30 s)**, calm stays 20 s with the 200 m earshot. No callout fires while nobody
  is wanted.
- `RadioLine` (scanner log) now records the `tier` and `duration_s` (serde defaults keep old logs
  readable).

### 3. Real information (coordination with machine E)
I did **not** add a parallel search target. The lost-visual call reads the shared racer record —
`RacerRecord::last_known` (+ the velocity when last seen) — which is exactly what search and
interception steer to: on `lead/round12` units intercept a point predicted from `last_known`
along the last heading; on E's `ws/police` (rows 61/63/64/66) units go to search points around
`last_known` / `last_known_vel`. The "copy, searching on X" lines name the intercept point they
were actually given. The test checks that the intercept points sit around the called position
or ahead of it along the called heading, which holds for both. Chatter is an observer: no
simulation state, RNG or hash is touched (goldens unchanged).

**For E (merge notes):** my hooks in files you are changing are small —
`lib.rs`: `pub mod chatter`, a `chatter` field + init, `chatter::attach_world(&mut p, world)`
right after `with_city` in `for_world`, `let thought = …; self.chatter_step(world, events,
thought)` before `flush_radio`, `level` instead of `any_wanted` in `flush_radio`;
`dispatch.rs`: `racer_desc` body, the Pursue / Intercept / RoadblockAt / SpikeAt arms of
`radio_text`, "Unit N," instead of "Unit N:" in two lines; `pursuit.rs`: the level-up radio
call removed from `update_racer_rules` (chatter raises it) and the end-of-pursuit texts. If you
add events worth a callout (a ram, a junction block, a stuck reset), push them to `pending` as
you do and add a match arm in `chatter_events`; `Callout` has room. Your `covers` and my
`Tunnel` list are the same data; one can go once both are merged.

### 4. Browser (`apps/testbed-web`, `apps/ui-kit`)
- **Radio strip** (`src/radio_strip.ts`, `#radio-strip`): last 3 transmissions, each with a
  coloured call-sign chip (dispatch amber, air cyan, units by number on golden-angle hues), fades
  after 7 s. Lines arrive when *heard*: from the scanner's `RadioSay` (in sync with the voice,
  after the scanner tier delay), or straight from police events while the audio is not running
  or the tier is "None". Placed by `HudLayout` under the chase readout / weather line, above the
  pursuit banner and minimap. The chase readout's own radio lines are hidden in the testbed (CSS)
  so the same line is not shown twice.
- **Voice** (`src/audio/scanner.ts`): speechSynthesis with a per-unit system voice (English
  preferred, fixed hash), per-unit pitch (wp_audio's) and rate, faster for pursuit traffic;
  dispatch always the first voice. Squelch on/off clicks and the static bed (`radio_rec` when
  shipped) go through the existing radio EQ (`mixer.radioIn`). **Limitation:** the browser's
  speech output cannot be routed into Web Audio, so the spoken words themselves are not
  band-passed; their volume follows master × Police bus and mute. Without speechSynthesis or with
  the switch off a line is squelch + static + strip text.
- **Settings:** game menu → Audio → "Police radio voice" (on/off, `GameSettings.radioVoice`,
  persisted); volume = the existing "Police radio" slider (Police bus). Also
  `__wpAudio.setRadioVoice(on)`, `__wpAudio.speech()`, `__wpAudio.hearsRadio()`,
  `__wp.radioStrip`. Audio never feeds the sim.

## How to test
| What | Command |
|---|---|
| Director: one at a time, priorities, supersede, expiry, L1 spacing, more chatter per level | `cargo test -p wp_police --lib radio` |
| Scripted pursuit: 10-80 → lost visual (place + heading = `last_known`) → units sent there → eyes on → custody, no overlap; L1 rate limits; quiet on patrol; place phrases; Port Vesper street + tunnel names | `cargo test -p wp_police --test radio_chatter` |
| Radio budget scenarios (row 6) | `cargo test -p wp_police --test police_scenarios` |
| Pacing = scanner speaking rate | `cargo test -p wp_audio --test director radio` |
| Read a whole chase | `cargo run --release -p wp_police --example radio_log -- crates/wp_police/scenarios/radio_city_chase.ron` |
| Browser: strip shows lines (events path and scanner path), call-sign chips, speech attempted when supported, voice switch | `npm run e2e:radio` (part of `just web-e2e`) |

## Numbers
`radio_city_chase.ron` (4 × 5 min night chase on the 12×12 grid), lines per minute:

| | In pursuit | Nobody wanted |
|---|---|---|
| Budget (row 6) | ≤ 10 | ≤ 4 |
| `lead/round12` (row-6 fix) | 5.6 | 3.4 |
| This branch | 6.8 | 3.4 |

Perf: chatter runs per step over the step's events and on think ticks over racers × units
(distance checks); `place_phrase` (a linear nearest-road scan) runs only when a callout is raised.
Not measurable in `police_bench` noise.

## Gate (this branch)
fmt, `clippy --workspace --all-targets -D warnings`, wasm build, ui-kit vitest + tsc, testbed tsc,
`npm run e2e:radio`: clean. `cargo test --workspace` / `just scenarios`: the only failures are
already red on `origin/lead/round12` with identical results — 20 goldens (same new hashes on
both: this branch does not change the simulation), `wp_race` race_picker (hollins / loop9 /
lantern), `wp_opponent` live police chase, and simcli `police/tunnel_escape_founders_cut{,_control}`
(capture/escape timing, same hashes on both; E re-aims them on `ws/police`).
`just web-e2e`: `race.mjs` and `radio.mjs` pass; in `run.mjs` only the software-GL perf budgets
(`perf_check.mjs`: real-time factor, long frames, KB/frame, material bumps) fail, and they fail
the same way on the base in this container. One `crash_fx.mjs` skid-mark count (6 < 8) failed
once under build load and passed on two re-runs.

## Known gaps
- Speech is not band-passed (browser limitation, see above); recorded line packs would fix it.
- The procedural grid has no street names (district phrases only). Port Vesper has names for every
  road; cities loaded from `.road.bin` have none until the road format carries them.
- Text and voice follow the scanner tier delay (Pro 6 s by default) — that is the scanner gadget's
  design; the strip waits for the voice so they stay together.
- Racers are still "racer #N" in dispatch lines (multiplayer needs the distinction); unit lines say
  "him".

## Changes outside my crates
- `wp_world`: `World::road_names` / `set_road_names` / `edge_names`, `roads::edge_roads`;
  `World::vesper` uses `generate_with_report` (same network). Presentation only, not hashed.
- `wp_audio`: one test (`radio_pacing_matches_the_scanner_speaking_rate`).
- `apps/ui-kit`: `GameSettings.radioVoice` + the menu toggle + a unit test.
