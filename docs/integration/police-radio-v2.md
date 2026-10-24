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

---

# Row 144: radio priority tiers (`docs/briefs/police-radio-priority.md`)

- **Branch:** `claude/project-thread-75jdte`, from `origin/lead/round12` (2bac2382).
- **Crates and folders touched:**
  - `crates/wp_police`: `radio.rs`, `chatter.rs`, `dispatch.rs`, `pursuit.rs`, `captain.rs`, `lib.rs`; new example `radio_bench`; new scenario `radio_race_pack.ron`.
  - `data/police/radio.toml`.
  - `crates/wp_audio`: scanner, director and types.
  - `crates/wp_wasm`: `audio.rs`.
  - `apps/testbed-web` and `apps/ui-kit`.
- **Contract version built against:** 0.3.0, unchanged. See request R144-1 below.
- The tier column of the row 79 table above is superseded by the table below. The old names still parse from saved logs: `Critical` is read as `Escalation` and `Tactical` as `Investigation`.

## Tiers

`RadioTier` is ordered so that a higher tier wins. `RadioTier::number()` gives the brief's numbering, 1 to 5.

| Tier | Lines | Pacing |
|---|---|---|
| 1 Actionable | Roadblocks and spikes, announced when planned, with distance and direction ("Dispatch, racer #7: roadblock 650 metres ahead, southbound on X; spikes 300 metres ahead"). Air unit launching, eyes on, lost and reacquired. A unit moving to intercept *ahead* of the racer, at L2 and up, with distance. | No gap. It waits only for the transmission on air, not the silence after it. Cuts any lower tier, including emergency break-ins. A tool warning cuts other tier 1 traffic. Tools planned in the same step for the same racer are spoken as one line. |
| 2 Escalation | 10-80, level up (with the department taking over and the rams now authorised), level down, pursuit end, witness lines, captain priority. | No gap. Cuts tiers 3 to 5. |
| 3 Impact | Contacts picked by severity, wrecks (two stacked lines), unit down, spikes hit, PIT attempt and verdict, assault on an officer (a break-in), a pursued racer hitting a civilian. | `impact_gap_s` 0.4 s, `impact_min_gap_s` 0.8 s after the line. Short life (`impact_stale_s` 4 s). Heavier impacts live longer: 6 s, and 8 s for a wreck. |
| 4 Investigation | Call announcements, "units responding" (once per racer at L0 to L1, before any pursuit), lost visual, eyes on again, searching, tunnel, box-in, bumper, helicopter "still on", nobody to send. | The global gap per level, as in row 79. |
| 5 Routine | Patrol chatter, out of service, backup requests. | `routine_gap_s` [10, 14, 20, 30, 45] s by level, with a gap that grows as heat rises. `routine_quiet_s` 6 s after any important line. Dropped first: a routine line is removed as soon as anything higher is waiting. |

**Removed deploy lines.** The removed lines are:

- "copy, sending unit n of m";
- "Unit 9 in pursuit of …";
- "moving to intercept" behind the racer, or at L1;
- "setting up a roadblock / deploying spikes on …" after the fact;
- the helicopter's track assignment.

`radio_deploy_suppressed()` counts them for the bench. At L0 to L1 the only deploy-style line is one "Dispatch: units responding, racer #N …" summary.

**Takeover lines** come from `takeover` in `radio.toml`, indexed by level:

- 3: "Highway Patrol is taking over the pursuit."
- 4: "Heavy units authorised."
- 5: "Tactical units authorised, all units, stop him by any means."

The rams sentence ("Hard rams and rolling blocks authorised.") is read from `pursuit.chase.hard_ram_level` and `rear_ram_level`, so the radio never promises rams the AI will not use.

## Impact severity

`chatter::impact_severity` measures collision energy as ½ · impulse · closing speed, or uses a nominal 1500 kg car when the contact has no impulse.

| Severity | When | Line |
|---|---|---|
| Tap | Below `hit_j` (6 kJ) | No line |
| Hit | Up to `heavy_j` (30 kJ) | "I'm hit! I'm hit!" |
| Heavy | Above 30 kJ, or health ≤ 0.45 | "He rammed me, I've lost my front end!" |
| Wreck | Only a unit the damage model wrecked | "Officer down" and "I'm done!" |

Energy alone stops at Heavy. The full-physics race showed why: a 110 kJ hit gave "officer down", and that unit then made an arrest.

**Rules for picking the line:**

- Each role (victim, rammer, self-crash) has its own pool.
- "Officer down" exists only in the Wreck pool.
- Every tier 3 line is shouted, ending in "!".
- A wreck stacks a second line from the nearest unit on the same racer, or from dispatch.
  - Both lines use `WRECK_PRIO` (5, above every other impact line).
  - The reaction lives 12 s, so it follows its victim line even when tier 1 calls cut in between.
- A unit with no racer to chase does not narrate its own scrapes.

## Browser

- **Scanner (`wp_audio`):** `RadioLine.tier` and `RadioSay.tier`, both with serde defaults.
  - The tier maps to a priority: 1 Urgent, 2 Pursuit, 3 Dispatch, 4 Routine, 5 Chatter.
  - Tier 1 lines skip the scanner tier delay.
  - With `set_important_only`, tiers 4 and 5 are not voiced.
  - The bridge gets the tier per police event from `PoliceSystem::radio_tier_of`, through `wp_wasm`'s `frame_commands`.
- **Owner decisions:**
  - "Tier 1 skips the delay, but make sure the map notifications and the scanner work at the same time." The roadblock and spike markers come from `map_dynamic`, which reads `roadblocks()` and `spike_strips()` every frame. The tool exists in the frame its decision is made, which is the frame its tier 1 line is spoken, so the marker and the voice appear together.
  - "Drivers all male, dispatch female." `pickVoice` picks voices by name: a female system voice for dispatch, and a male one per unit, hashed by unit. Pitch: dispatch 1.15, units low.
- **Sound:**
  - Tier 1 plays a two-tone cue (1250 then 1650 Hz, 0.11 s each) into the radio EQ, and the voice starts after it.
  - Tier 3 is spoken faster (rate +0.25) and higher (+0.12), with a louder squelch and static.
- **Strip:**
  - Each line gets a `tier-N` class and a `data-tier` attribute.
  - Tier 1 shows ⚠, bold and amber. Tier 2 shows ▲, bold. Tier 3 shows ✸, in red.
  - Tiers 1 and 2 stay on screen 12 s. Tiers 4 and 5 are dimmed.
- **Setting:** game menu → Audio → "Radio: All / Important only".
  - Stored as `GameSettings.radioFilter`, default "all".
  - Important only drops tiers 4 and 5 from both the voice and the strip.

## How to test (row 144)

| What | Command |
|---|---|
| Tier 1 cuts routine; routine dropped when busy; tier ordering; wreck stack; old logs parse | `cargo test -p wp_police --lib radio` |
| No deploy lines at L0 to L1 except one summary; L3 tool line within 1 s over other traffic; severity picking; takeovers | `cargo test -p wp_police --test radio_chatter` |
| Tier 1 skips the scanner delay; important-only; tier to priority | `cargo test -p wp_audio` |
| Bench and transcripts | `cargo run --release -p wp_police --example radio_bench -- crates/wp_police/scenarios/radio_city_chase.ron [--log 4] [--text-tiers]` |
| Browser: tier on strip and scanner lines; filter switch; default All | `npm run e2e:radio` |

## Bench (before = `lead/round12` with text-classified tiers, after = this branch)

How the bench counts:

- Lines per minute by the highest wanted level of any racer at the time.
- "Deploy" means unit deployment chatter: en route, responding, in pursuit of, moving to intercept, sending unit, setting up a roadblock, and similar. Call announcements and the "units responding" summary are not counted as deploy lines.
- Delay is the time from a `RoadblockPlaced` or `SpikesPlaced` decision to the first line about it, by the racer's level at the decision. A decision with no matching line within 20 s counts as never spoken.

**`radio_city_chase.ron`** (4 × 5 min night chase):

| lines/min | L0 before | L0 after | L1 before | L1 after | L2 before | L2 after | L3+ before | L3+ after |
|---|---|---|---|---|---|---|---|---|
| T1 | 0 | 0 | 0 | 0 | 0 | 11.4 | 2.9 | 3.5 |
| T2 | 0 | 0 | 13.5 | 13.5 | 3.8 | 0 | 0.4 | 0.3 |
| T3 | 0 | 0 | 0 | 0 | 0 | 0 | 1.7 | 2.3 |
| T4 | 4.8 | 4.8 | 0 | 0 | 0 | 0 | 1.0 | 1.4 |
| T5 | 0 | 0 | 0 | 0 | 0 | 0 | 1.1 | 0 |
| all | 4.8 | 4.8 | 13.5 | 13.5 | 3.8 | 11.4 | 7.1 | 7.5 |

Minutes at each level: 0.8 / 0.3 / 0.3 / 18.6. Deploy lines per minute (L0 / L1 / L2+): before 0 / 0 / 2.2, after 0 / 0 / 0.2.

| Deploy decision → spoken | before: n, median, max | after: n, median, max |
|---|---|---|
| L2 | 2, 8.8 s, 8.8 s | 3, 0 s, 0 s |
| L3 | 1, 7.4 s, 7.4 s | 4, 3.6 s, 3.8 s |
| L5 | 6, 8.1 s, 13.0 s | 23, 0 s, 0 s |
| Never spoken | 21 | 0 |

**`radio_race_pack.ron`** (new: four racers at 2.8× the limit across the city, 3 × 4 min). Deploy lines per minute at L2+: 1.6 before, 0.7 after.

| Deploy decision → spoken | before | after |
|---|---|---|
| L2 | none | 2, 2.5 s, 2.5 s |
| L3 | 1, 13.6 s | 4, 0 s median, 4.7 s max |
| L5 | 2, 10.6 s | 20, 0 s median, 1.9 s max |
| Never spoken | 23 | 0 |

The L3 cases over 1 s are a second tool decided while the first tool's warning is still on air. The second waits for that transmission, and a tool line does not cut another tool line.

**Race with police, full physics** (`city_sprint_police.ron` through `wp_race`'s `race_police` harness, extended to 150 s):

- The race only reaches L1 to L2: 1.2 min at L0, 1.3 min at L1, under 0.1 min at L2. So it has no tool decisions.
- Lines per minute at L1: 7.9 before and 8.7 after.
  - T5 (the "in pursuit of" line) goes from 0.8 to 0.
  - T3 goes from 0 to 1.6 (two contact impacts).
- Deploy lines at L1: 0.8 per minute before, 0 after.
- See transcript 5.

**Perf:**

- Chatter does no more work per step than in row 79. Tool lines are built only in steps that decide a tool, and impacts only from the step's contacts.
- The director's flush is linear in the held lines, which is fewer than 10 in practice.
- None of this shows up in `police_bench` noise.

## Transcripts

See [police-radio-row144-transcripts.md](police-radio-row144-transcripts.md). It has five excerpts:

- a roadblock and spike sequence;
- a department takeover;
- heavy units and a wreck pair;
- a wreck between tier 1 calls;
- the full-physics race, before and after.

## Gate (row 144)

- `cargo fmt` and `cargo clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo test -p wp_police -p wp_audio`: all pass.
- `cargo build --target wasm32-unknown-unknown -p wp_wasm` and wasm-pack: OK.
- testbed `tsc`: clean.
- ui-kit vitest:
  - 110 of 111 pass. `data.test.ts` "stock builds are the measured table" fails (247.9 > 248).
  - It fails the same way on `lead/round12` without this branch.
- `npm run e2e:radio`: passes, including the new tier and filter checks.
- `npm run e2e:siren`: passes.
- Goldens: none move. `simcli goldens` gives the same results and the same hashes on `lead/round12` and on this branch: 112 ok, and 3 already red on the base (`jam_pursuit_grid`, `police_tunnel_escape_founders_cut`, `police_tunnel_escape_founders_cut_control`). Nothing was re-blessed. The radio is presentation and is not hashed.

## Known gaps (row 144)

- **Tool warnings back to back.** A second tool warning waits behind a first one still on air, up to about 4.7 s. Making tool lines cut each other would chop the first warning mid-sentence.
- **No real-physics contacts in the police harness.** Its impact lines come from `UnitDisabled`, spikes and PIT. The severity picking from real contacts is exercised by the unit tests and the full-physics race only.
- **Wrecks can still go stale under a flood.** At L5 with four racers, wrecks happen faster than one channel can speak them. Wreck pairs win over other impact lines, but the rest still go stale.
- **Wanted level column.** The 10-80 can still go stale (15 s) behind a run of tier 1 lines. The bench's "level" column is the highest of all racers, so the race pack shows L5 while one racer is at L3.
- **Old wording for the tier 4 summary.** Before the first pursuit, "Dispatch: caller reports … Unit 2 responding" still names the unit sent. It is the call announcement (tier 4), not a deploy line. Only the extra "copy, sending" lines were removed.

## Contract change requests

**R144-1: carry the tier on `PoliceEvent::Radio`.**

- **Proposal:** add `tier: u8` (1 actionable … 5 routine, 0 = unknown) to `PoliceEvent::Radio`, with `#[serde(default)]`.
- **Why it is needed:**
  - Today the tier lives only in `wp_police`'s `RadioLine` log.
  - The single-player browser looks it up with `PoliceSystem::radio_tier_of(unit, text)`.
  - A networked client only receives the event, so it cannot know the tier. Its strip styling, tone cue, delay skip and important-only filter would fall back to tier 0, which is unstyled and filtered like tier 4.
- **What changes if accepted:**
  - `flush_radio` fills in the field.
  - `wp_wasm` and `wp_net` read it, and `radio_tier_of` goes away.
  - The `wp_audio` director takes it straight from the event.
- **Cost:** one field. Not hashed (the radio is presentation).

## Voice upgrade (listed, not built)

What it would take to replace browser text-to-speech with recorded or generated lines:

1. **Voice cast.**
   - 6 to 8 distinct male officer voices, hashed by unit as `pickVoice` does today.
   - 1 to 2 female dispatchers.
   - 1 to 2 air crew voices with rotor noise baked in.
   - Each needs calm and shouted takes.
2. **Line packs instead of free text.**
   - Today's lines are templates, with a call sign, place, distance, racer number and heading.
   - Recorded audio needs these split into phrase slots, for example "Unit" + "twelve" + ", roadblock" + "six hundred fifty" + "metres ahead" + "southbound" + "on" + street name.
   - That means:
     - a phrase table in `data/police/voice/*.toml`;
     - a stitcher in `wp_audio` that emits a phrase list instead of text;
     - recorded numbers (0 to 99, hundreds, thousands);
     - recorded street and district names (Port Vesper has about 120).
   - For the procedural grid, districts only.
   - Generated TTS can render the packs offline, per voice and per take.
3. **Shouted tier 3 variants.**
   - Separate takes for each impact pool line ("I'm hit!", "officer down", "I'm done!") per officer voice.
   - Breathing and engine-strain layers.
   - About 40 lines × 8 voices.
4. **Radio processing chain in Web Audio.** Recorded audio can go through `mixer.radioIn`, which browser speech cannot. The chain:
   - 300 to 3400 Hz band-pass;
   - soft clip and saturation;
   - a 4 to 8 kHz noise bed;
   - a squelch tail;
   - per-tier gain (tier 3 hot, tier 5 low).

   This is the chain the row 79 notes could not apply to speech.
5. **Streaming and memory.**
   - About 2,000 to 3,000 phrase clips at 16 kHz mono Opus, about 15 to 25 MB.
   - Lazy-load by district.
   - Keep the tone and squelch procedural.
6. **Timing.**
   - Clip lengths replace the `words / 2.6 + 0.7` estimate.
   - The director must take a measured duration per line. This is a small API change in `wp_police::radio`, which today estimates duration from the text.
7. **Fallbacks.** Missing phrases fall back to TTS for that slot, or to strip text only.

## Changes outside my crates (row 144)

- `wp_audio`:
  - `RadioLine.tier` and `RadioSay.tier`, with serde defaults;
  - `priority_for_tier` and `important_only` in the scanner;
  - `set_police_radio_tiers` and `set_radio_important_only` in the director;
  - tests;
  - `tier: 0` added in `examples/mix_trace.rs`.
- `wp_wasm` (`audio.rs`): tiers passed to the director, `audio_set_radio_important_only`, `police_radio_tier`.
- `apps/testbed-web`: scanner voices, tones and tier speech; strip styling; the important-only filter; `e2e/radio.mjs` checks.
- `apps/ui-kit`: `GameSettings.radioFilter`, the menu row, a test.

---

# Radio relevance and plain speech (owner, 2026-10-23)

Owner: "make sure police chatter and audio is cleaned up and simplified — generally speaking
police should not be engaging random AI cars on the other side of the map, and it definitely
should not be polluting the radio channel." Audit and probes: session `audio` (phase A report).
Police *behaviour* toward AI racers is the cops workstream; this section is the radio only.

## What changed

- **The radio is the player's** (`PoliceSystem::set_listeners`). `wp_sim` sets the listeners to
  the `VehicleKind::Player` cars every step (`police_link.rs`, one call; with a player cop seat
  everyone listens). Every `RadioCue` knows the racer it is about (`RadioCue::about`, or
  `PoliceSystem::radio_subject`: the racer the text names, else the speaking unit's target, else
  the call it answers) and optionally where it happens (`RadioCue::at`). The director drops a
  held line unless it concerns a listener: about a listener, or the racer / place / speaking
  unit within `relevant_m` (300 m) of one; impact lines only within `impact_m` (250 m)
  (`RadioStats::irrelevant` counts them). Pacing (gaps by level, line budget) follows the
  listeners' level and state, not the hottest AI chase on the map. Headless scenarios without
  listeners hear everything, as before; a scenario can name them (`listeners: [0]`).
- **Line budget** (`max_lines_per_min = [2, 4, 6]`, calm / wanted or searching / pursuit): tiers
  3–5 wait while the last 60 s already had that many lines; tiers 1–2 always go out.
- **No routine chatter** (`routine = false`): "clear and available", "resuming patrol", crash
  scenes, backup requests.
- **Plain speech.** The single listener is "the suspect" (`radio::name_suspect`; other racers
  keep "racer #N"). Gone: "10-80", "Code 4", "Resume patrol", "EMS", "PIT" ("going to spin him
  out"), "Units, check the cover", "Spotlight on". Tool warnings are "Dispatch: roadblock 650
  metres ahead, southbound on X." Level-up: "Dispatch: the suspect is now level 3. 4 units on
  him. Highway Patrol is taking over the pursuit. Hard rams and rolling blocks authorised."
- **Fewer low-value lines.** No per-unit "copy, searching …" lines; no helicopter position
  reports ("still on him, 81 km/h"); "on his bumper" once per racer per 60 s (was 12 s), without
  the place.
- **Audio.** `wp_audio`'s own spike-hit radio line is off (`FlatTyreConfig::radio_line`, the hit
  was heard twice); a released tier 3 line not spoken within 3 s is dropped
  (`scanner::IMPACT_STALE_S`). Testbed: speech still running when the next line starts is cut,
  never queued behind (the voice drifted behind the strip); a cut tier 1 line no longer speaks
  after its alert tone; a new session resets the audio director, speech and the helicopter rotor
  (`AudioSystem::resetSession`); the rotor loop stops after 5 s with no helicopter in range.
- **HUD feed (ui-kit `reduce.ts`).** Violations, pursuit start / end, spikes, PITs, air launches
  and wanted levels of other racers are no longer fed to the driver's scanner alerts / map
  incidents (`ReduceContext.allRacers` turns them back on for a captain view).
- **Contract (v0.5.0, applied by the contracts session):** `PoliceEvent::Radio` now carries
  `tier` and `racer`; `flush_radio` fills both.

## Numbers (probes in the audit session's scratch dir; police harness, lines per minute)

"Before" = main at ee742a68. "After" = this change on the shared tree, which also has the cops
session's uncommitted perception / dispatch work, so pursuit counts differ; the "no listeners"
column is the same tree with every racer listening (old relevance), to separate the two.

| Situation | Before: lines/min (about the player; about AI racers > 400 m away) | After, no listeners | After |
|---|---|---|---|
| (a) calm, nobody else misbehaving (`radio_calm_patrol`) | 0.0 | 0.0 | 0.0 |
| (b) player calm, 3 AI racers chased elsewhere (`radio_npc_far`) | 9.7 (0; 6.0), 81 % airtime | 7.5 (0; 5.3) | **2.1 (0; 0)**, 18 % airtime |
| (c) player's pursuit alone (`radio_city_chase`) | 7.5 (7.3; –) | – | **6.6** (6.5; –) |
| (c') player's pursuit + 3 AI chases (`radio_npc_mixed`) | 9.7 (2.4; 5.2), 79 % airtime | 7.2 (1.9; 3.4) | **6.8 (4.9; 0)** |
| (c'') player searching + AI chases | 12.1 (1.8; 4.9), 98 % airtime | 7.5 (1.6; 4.6) | **3.0 (1.3; 0)** |
| (d) 60 s after an escape (`escape_break_contact_night`, no stop) | not reached | – | **0.5** (the "we've lost the suspect" line) |
| searching, alone | 9.1–9.3 | – | 1.8–5.6 |

The AI-racer lines left in (b) / (c') are AI chases that pass within 300 m of the player.

Real game (native `wp_wasm::Session`, Port Vesper, 13 street racers, 30 min of sim time; the
"after" tree also has the cops session's AI-racer scope, `npc_scope_m`):

| Run | Before | After |
|---|---|---|
| 22:00, player cruising | 18 lines (0.6/min), 11 about AI racers (10 of them > 400 m away), 2 map-wide AI pursuits (a roaming racer rammed a patrol 568 m / 934 m away) | 1 line: "caller reports reckless driver, on Larkspur. Unit 7 responding." (an AI racer 152 m from the player); no AI pursuits |
| 21:00, player parked | 1 line | 1 line (an AI racer near the player) |

## Tests

`cargo test -p wp_police --lib radio` (relevance, budget, routine off, "the suspect"),
`--test radio_chatter`, `--test police_scenarios` (new `radio_npc_far.ron`, `radio_npc_mixed.ron`),
`cargo test -p wp_audio` (spike hit without a second line, late impact dropped).

## Known gaps

- Networked clients: the server's listeners are all players; per-client filtering needs the
  client to filter on `PoliceEvent::Radio.racer` (now in the contract) — `wp_net` follow-up.
- "I just watched the suspect … I'm on it" and dispatch's "all units, pursuit of the suspect"
  ~9 s later say the same thing twice.
- Pursuits that flap (lost / restarted every 20–40 s) each get a start and an end line; seen with
  the cops session's perception work in progress (62 pursuits in 70 min of
  `escape_break_contact_night`, 15 before) — theirs to look at.

## Contract change request (filed earlier as R144-1, extended)

`PoliceEvent::Radio { unit, text, tier: u8, racer: Option<EntityId> }` (`#[serde(default)]`):
`tier` 1 actionable … 5 routine (0 unknown); `racer` the racer the line is about. Needed for
per-client filtering over the network (each client keeps lines about its own racer, or with a
place near it) and replaces `PoliceSystem::radio_tier_of` text look-ups. Status 2026-10-24: being
applied by the contracts session (v0.5.0); `wp_police` fills both fields.
