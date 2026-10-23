# 5-minute single-player greybox demo

**Who this is for:** the owner and every machine working toward Checkpoint 2 (Greybox).
**What it is:** one player, one car, the procedural test city at night, live traffic and physical
police. Drive, raise heat, get chased, escape or get caught, pay the repair bill.
**Done means:** someone who has never seen the project plays the published testbed for five
minutes, without help, and goes through every beat below. See [What "done" looks like](#what-done-looks-like).

This replaces nothing: `DEMO_SCRIPT.md` is the 3-minute *presenter's* script for a live show.
This file is the *build target* for the greybox: what the build must do and who still owes what.
After playing, fill in `PLAYTEST.md`.

All numbers below come from the data files on `main` (`data/police/*.toml`,
`data/economy/*.toml`, `data/city/procedural_12x12.road.json`) as of 2026-10-19. If a table
changes, the data file wins; update this page.

---

## 1. Setup

| Setting | Value | Where it comes from |
|---|---|---|
| Entry point | **night run** button in the testbed's scenario panel | `apps/testbed-web/src/main.ts` `startNightRun` → `wp_wasm::night_run_json` |
| City | `procedural_12x12`: a 12 × 12 grid, 110 m blocks, about 1.5 × 1.5 km of streets | `data/city/procedural_12x12.*` |
| Start | City centre, on the north–south highway, facing north, **already doing 108 km/h** (30 m/s) | `wp_wasm::scenario::city_chase` (`pos = [0,0,0]`, `CITY_CHASE_SPEED_MPS`) |
| Clock | 23:00, running 10× (five real minutes ≈ 50 game minutes, so night the whole demo) | `city_chase`: `time_of_day_h = 23`, `time_ratio = 10` |
| Seed | 23: the same traffic and patrols every run, so runs can be compared | `city_chase` |
| Police | full presence (`police_presence = 1.0`), police cars are real sim vehicles | `wp_sim/src/police_link.rs` |
| Car | **Shiro Turbo** (`shiro_turbo`, RWD sport, 1301 kg). Pick it in the car box *before* pressing city chase | `data/cars/ROSTER.md` |
| Damage mode | **Arcade** (survives a hit or two; repairs at 0.5× price) | `data/modes/arcade.toml` |
| Assists | the keyboard or gamepad preset the testbed applies automatically | `applyAssistPreset` |
| Camera | chase camera (set by the preset) | — |

### The city at a glance (measured from the road graph)

| Road | Length | Limit | Lanes | Use in the demo |
|---|---|---|---|---|
| Central highway (N–S, x = 0) | 1.49 km end to end | 88 km/h | 3 each way | Start here. Speed builds heat; long sight lines. |
| Ring road (closed loop round the grid) | **5.58 km** per lap | 72 km/h | 2 each way, chamfered corners | Escape loop: police are capped at 115 km/h at night, you are not. |
| Arterials | — | 56–64 km/h | 2 each way | Chase roads with traffic and signals. |
| Local streets | 36.6 km of them | 40 km/h | 1 each way, parking lanes | Breaking line of sight: buildings on every block. |

58 signalised junctions. Two police stations (Precinct 1 at (−220, 295) with the helipad,
Precinct 2 at (330, −255)), three garages, four safehouse candidates.

## 2. Controls

| Action | Keyboard | Gamepad |
|---|---|---|
| Throttle / brake | W / S or ↑ / ↓ | right / left trigger |
| Steer | A / D or ← / → | left stick |
| Handbrake | Space | A or X |
| Gear up / down | Shift / Ctrl | RB / LB |
| Horn | Q | Y |
| Headlights | O | — |
| Reset car | R | — |
| Pause / step | P, `,` and `.` | — |
| Cameras (chase, hood, bumper, orbit, cinematic) | 1 … 5 | — |
| Repair bill | **F** | — |
| Help | F1 or / | — |

From `apps/testbed-web/src/input.ts` and `main.ts`. The demo needs only drive, handbrake, reset
and **F**.

---

## 3. The five minutes, beat by beat

"**On main?**" says whether the beat works on `main` today. The owner of each gap is in §5.

### 0:00 – 0:30 · Launch
Press **city chase**. You are already flying north on the highway at 108 km/h at night, with
traffic around you.
- **Player learns:** this car is fast and heavy; the city is lit and moving.
- **On main?** Yes.

### 0:30 – 1:15 · Heat rises
Keep speeding, run a red light on an arterial, clip a civilian if you like.

How heat adds up (night):

| Event | Heat if a unit sees it | If nobody sees it (a "report", × 0.2) |
|---|---|---|
| Speeding | 2 + 0.4 per m/s over the limit, at most once a second. 108 km/h on the 88 km/h highway ≈ **4.2 per second** | ≈ 0.8 per second |
| Red light | 6 | 1.2 |
| Wrong way | 4 | 0.8 |
| Collision | 8 | 1.6 |
| Reckless driving | 3 | 0.6 |
| Disabling a police car | 15 | — |
| Being chased (seen) | +0.25 per second | — |

A unit *witnesses* a violation within **70 m** at night (160 m by day) with line of sight.
Wanted levels start at heat **10 / 30 / 50 / 70 / 90** (L1–L5). Speeding past one patrol car
for three seconds is enough for **L1**. An unseen report sends a unit after **50 s** at night.

- **Player learns:** police react to what they see. Heat is a meter you fill yourself.
- **On main?** Yes in the sim and in the heat HUD. Two heat readouts are drawn (C's panel and
  M's HUD); they must become one.

### 1:15 – 2:45 · The chase
A unit has to *recognise* you before it pursues: at night that is **4 s of continuous sight
within 40 m** (by day 0.3 s within 250 m). A glimpse at night is not enough.

What the police do at each wanted level:

| Level | Heat | Units on you | Tactics and tools unlocked | Reinforcements (night) |
|---|---|---|---|---|
| L1 | 10 | 1 | Lead unit tails; everyone else **shadows** 25–40 m back | 60–150 s |
| L2 | 30 | 2 | + **PIT** (Arcade ≤ 70 km/h, needs 60 m of straight), **box-in** (≤ 60 km/h), **spike strips** 150–500 m ahead | 60–150 s |
| L3 | 50 | 3 | + **roadblocks** 300–700 m ahead (highways allowed up to L3), **rolling block**, **helicopter** (launches after 120 s at night, 25 % chance it is unavailable) | 60–150 s |
| L4 | 70 | 4 | everything; no roadblocks on the highway above L3 | 60–150 s |
| L5 | 90 | 6 | everything | 60–150 s |

Rules that make the chase fair:
- Pursuers never drive faster than you + 10 %, and never above **115 km/h at night** (144 km/h by day).
- They spend a *risk budget*: hard cornering, near misses and contact cost budget; when it runs
  out they ease off.
- Police cars take damage like yours. A unit that drops to 40 % health disengages.
  In Simulation one hard hit ends it.

A 5-minute demo will normally reach **L1–L2**. Reaching L3 takes roughly 50 heat: a long seen
chase with several violations.

- **Player learns:** chases are tense, not bumper cars. Contact costs both sides.
- **On main?** Partly. The pursuit runs, police are drawn with light bars, sirens play. **Police
  cars still ram stopped cars** and spawn too close to other vehicles (E's top gap). Sirens are
  set by hand, not by pursuit state (J/lead) — on `lead/demo-endrun` they follow each unit's
  assignment (lights and siren on while responding or pursuing, off on patrol).

### 2:45 – 3:45 · Breaking contact
Get out of every unit's line of sight and stay out:

| Wanted level | Night (× 0.7) | Day (× 1.0) |
|---|---|---|
| L1 | **8.4 s** | 12 s |
| L2 | 11.2 s | 16 s |
| L3 | 15.4 s | 22 s |
| L4 | 21 s | 30 s |
| L5 | 28 s | 40 s |

Ground units see 140 m at night (headlights), the helicopter 350 m. Good ways to break contact
in this city: turn into the local streets (buildings on every block hide you), or take the
ring road flat out (police are capped at 115 km/h). After escape, heat starts to fall 5 s
after the last sighting at **0.7 per second** at night, faster in the industrial (× 1.5) and
suburb (× 1.6) districts. A safehouse (stop 10 s, nobody within 150 m) cuts heat to 25 %.

- **Player learns:** you win by breaking contact, not by being faster.
- **On main?** The escape timer runs in the sim (`police_status().cooldown`). **No on-screen
  banner** says SEARCHING / ESCAPED.

### 3:45 – 4:30 · Ending: escape or capture
- **Escape:** the escape timer fills, the pursuit ends (`PursuitEnded { Escaped }`), the HUD
  shows **ESCAPED**, the wanted level stays until heat decays. Keep driving or head to a garage.
- **Capture:** below **7 km/h (2 m/s) for 4 s** with a police car within **15 m**. The pursuit
  ends (`PursuitEnded { Captured }`), the HUD shows **CAPTURED**, the car stops, the run goes to
  the bill.
- **Total-loss capture:** the car is wrecked when caught (`CapturedTotalLoss`). Arcade: still
  impounded, not destroyed.

- **Player learns:** both endings are fair and readable.
- **On main?** **No.** On `lead/demo-endrun`: yes — the capture is billed by G's economy, the car
  brakes to a halt and is held, the end-of-run panel and the bill open; an escape shows a bounty toast.

### 4:30 – 5:00 · The repair bill
Press **F**, or the bill opens on its own after capture. It shows:
1. **Damage**, part by part with labour and parts cost, scaled × 0.5 in Arcade.
   Options: **Repair all**, **Repair some**, **Drive it damaged**.
2. After a capture, **the fine**: L1 600, L2 1,200, L3 2,500, L4 5,000, L5 9,000, plus each
   session violation (speeding 40, red light 150, wrong way 200, collision 300, reckless 120),
   then × 0.6 in Arcade (× 1.25 in Simulation).
3. After a capture, **impound**: Sport class 700 base + 200 per day.
4. After an escape at night, **bounty** earned (2 per heat-second while chased).

Example: captured at L1 in Arcade after two red lights and one collision →
(600 + 150 + 150 + 300 + some speeding) × 0.6 ≈ **750 fine**, plus 700 impound, plus repairs.

- **Player learns:** every crash and every bust has a price that carries over.
- **On main?** Partly. The bill screen exists and prices real damage (`repair_quote_json`), but
  only by hand (**F**). On `lead/demo-endrun`: the end-of-run panel shows the fine, bail, impound
  fee, G's repair quote and cash before → after; **Repair & continue** pays impound + repair from
  the ledger and restores the car (`Sim::repair_vehicle`, a replay op). Bounty after an escape is
  shown as a toast.

---

## 4. What "done" looks like

The greybox demo is done when **all** of these hold on the published testbed:

- [ ] **city chase** starts in under 5 s on a normal laptop and holds 60 fps with traffic and police.
- [ ] Speeding past a patrol car raises heat and the wanted level on **one** HUD readout.
- [ ] A police car recognises you, pursues, and the HUD says **PURSUIT**.
- [ ] Police never ram a stopped civilian or a stopped player, and never spawn inside another car.
- [ ] Sirens switch on with the pursuit and off when it ends.
- [ ] Breaking line of sight shows **SEARCHING** with the escape timer; filling it shows **ESCAPED**.
- [ ] Stopping next to a police car for 4 s shows **CAPTURED**, the car stops and the bill opens.
- [ ] The bill lists damage per part, the fine and the impound fee, with the Arcade scaling.
- [ ] **Repair all** restores the car (dents gone, handling back) and takes the money.
- [ ] Same seed + same inputs = same chase (replay round-trips the whole demo).
- [ ] `golden/city_chase` passes in simcli with no police collision and with its checks no
      longer in `expect_pending`.
- [ ] The owner fills in `PLAYTEST.md` and scores the four core questions 3 or higher.

---

## 5. What is still missing, machine by machine

Cross-checked against `docs/STATUS.md`, every `docs/integration/*.md` (round 6 and 6b lead
feedback) and the code on `main` + the lead branch, 2026-10-19.
✓ = works today, ✗ = missing and needed for the demo, ○ = nice to have, not blocking.

| Machine | What works | Missing for the demo | Blocking? |
|---|---|---|---|
| **A** Vehicle | ✓ real handling, assists, keyboard/gamepad | — | no |
| **B** Damage | ✓ lattice crush, debris, collider follows crush, mechanical damage | ✗ `apply_repair(items, full)` and `summary()` (v0.3.0, round 6b task 1) so **Repair all** actually fixes the car | yes (last beat) |
| **C** Testbed | ✓ city-chase preset, police drawn with light bars, heat panel, bill on **F** | ✗ **PURSUIT / SEARCHING / ESCAPED / CAPTURED** banner from `police_status()`; ✗ open the bill on capture / end of chase; ✗ one heat readout (merge with M's) | yes |
| **D** World | ✓ city, traffic, signals, shared driving controller | ✗ leader / obstacle query so police stop behind stopped cars (with E) | yes (with E) |
| **E** Police | ✓ heat, dispatch, pursuit, tactics, escape/capture rules; units are sim vehicles on D's roads | ✗ **police ram stopped cars**; ✗ spawn clear of other vehicles. Done when `golden/city_chase` pursues without a collision | **yes, top priority** |
| **F** Net | ✓ server + prediction | — (single player) | no |
| **G** Economy | ✓ ledger, repair quotes, `settle_pursuit`, `handle_capture` | ✗ `EconomySession` glue: `SimEvent`s + damage report → bill, fine, impound, payout (round 6b task 1); ✗ impound flow end to end (task 2) | yes |
| **H** Cars | ✓ 13 cars incl. `sentinel_cruiser` | ○ cruiser pursuit tuning (catch every street car at L3+, survive ~3 rams); ○ 3 civilian traffic cars (today one model) | no |
| **I** Harness | ✓ `golden/city_chase` merged | ✗ re-aim the golden (the racer rear-ends a patrol car at 0.11 s; checks parked in `expect_pending`); ○ observer soak with police on, after E's fix | yes (proof) |
| **J** Audio | ✓ engine, tires, sirens, radio in the browser | ✗ sirens follow pursuit state (with lead); ○ radio lines from `POLICE_DOCTRINE.md` on pursuit start/end | partly |
| **K** Weather | ✓ crate | — (demo is dry) | no |
| **L** Race | ✓ rules, events, `city_sprint_police`; ✓ **step 2:** street races in the browser (**races** button: picker with 4 races on Port Vesper and the grid, AI racers, countdown, race HUD with minimap and next-gate marker, results + G's payout, capture → end-of-run panel) | ○ (step 2) reset onto the course instead of the grid slot; illegal-daytime heat strong enough to start pursuits (E) | no |
| **M** UI kit | ✓ chase HUD and repair bill mounted in the testbed | ✗ one heat / wanted / pursuit readout (with C); ✗ bill wired to G's quote + fine + impound | yes |
| **N** Opponent AI | ✓ AI racers | ○ racers react to police | no |
| **O** Meta | ✓ API | — | no |
| **P** Design docs | ✓ this file, `PLAYTEST.md`, `DEMO_SCRIPT.md` | ○ Port Vesper route lengths (blocked, see `CITY.md` §6) | no |
| **Q** Tuning dash | ✓ sweep runner | ○ pursuit sweep on aggression / ram parameters, findings to E | helps E |
| **Lead** | ✓ police as sim vehicles, physical traffic, city loading, `police_status` / `police_units` / `repair_quote_json` exports, typed v0.3.0 events. **Done on `lead/demo-endrun`:** ✓ drained `SimEvent`s + the player's damage feed G's `EconomySession` outside the sim step (`wp_wasm::run`, hashes unchanged); ✓ `run_result_json()` (outcome, bounty, fines, bail, impound + fee, G's repair quote, cash before/after) + `run_ledger_json()`; ✓ capture / wreck ends the run (car held on the brakes, result panel + bill, **Repair & continue** pays impound + repair from the ledger and repairs in place, **Restart** keeps the account); ✓ escape shows the bounty and play goes on; ✓ sirens and light bars follow each unit's assignment (respond / pursue / intercept / roadblock on, patrol off) | ○ refresh `STATUS.md` (Checkpoint 2 row, J row says audio "not wired" but it is); ○ a partial repair ("Repair some") from the end-of-run panel | no (merge) |

### Critical path
1. **E (+D):** police stop ramming → the chase is watchable.
2. **Lead + G:** `PursuitEnded` → `EconomySession` → end-of-run result exported to wasm.
3. **C + M:** one HUD, the banner, bill opens on capture showing fine + impound.
4. **B:** `apply_repair` so paying fixes the car.
5. **I:** re-aim `golden/city_chase` to prove 1–4 and keep them from regressing.

---

## 6. If something goes wrong while playing

| Problem | Do this | Note it for |
|---|---|---|
| A police car rams you or a civilian while stopped | Drive on; note the time | E |
| Heat never rises | Drive past a patrol car within 70 m; it must *see* you | E / C (HUD) |
| The chase never starts | Stay in sight for 4 s within 40 m (night rules) | — (by design; tell us if it feels wrong) |
| Two heat meters disagree | Use C's panel; note both values | C / M |
| Captured but nothing happens | Fixed on `lead/demo-endrun` (panel + bill open). Elsewhere press **F** for the bill | Lead / C |
| The car is wrecked in the first minute | Press **F** to see the bill, then **R** | — |
| Stutters | Close other tabs; note the moment | I (perf) |
