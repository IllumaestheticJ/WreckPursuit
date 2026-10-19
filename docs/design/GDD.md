# WreckPursuit — Game Design Document

**Owner:** workstream 19 (design docs). **Status:** v0, 2026-10-19, written against contract 0.1.0
and the code on `main` (vehicle, damage, world, economy, cars, harness merged; police, net, race,
audio, weather, UI in progress).

This document turns the engineering plan (`docs/DESIGN_OVERVIEW.md`, `prompts/*.md`) into the game
those systems add up to. Numbers marked **(code)** are already in data files on `main` and are
quoted, not invented; the source is named. Everything else is a design target for the owning
workstream to tune toward.

Companion docs in this folder: `CITY.md` (the map), `POLICE_DOCTRINE.md` (police behaviour and
radio), `CARS_LORE.md` (car fiction and cosmetics), `FEATURE_BACKLOG.md` (unscheduled ideas).

---

## 1. Pillars

1. **The physics is the truth.** Handling and damage come from one simulation, the same one the
   server runs. There are no scripted crashes, no invisible walls that save you, no "hero car".
   If it looks like it should hurt, it did.
2. **Damage has consequences you carry.** A crash costs money, time and handling. A car you could
   not afford to align pulls left on the next race. Clean driving is a skill and it pays.
3. **Chases are slow, calculated and dangerous for both sides.** Police win by containment and
   attrition, racers win by breaking contact. Nobody rams their way to victory without paying for it.
4. **The city is a place, not a backdrop.** Traffic has rush hours, districts have character,
   bridges are chokepoints, and the time of day changes who is out and what it pays.
5. **Fair by construction.** Same physics for everyone, no rubber-banding, no pay-to-win. Difficulty
   comes from the car, the driver, the time of day and the heat you chose to carry.

When a design question has no obvious answer, pick the option that best serves pillar 1, then 2.

---

## 2. Player fantasies

### 2.1 The racer
*"I know this city better than the people paid to patrol it."*

- **Verbs:** drive, race, choose a line through traffic, read the scanner, break line of sight, hide,
  repair or limp on, tune, bet.
- **Success looks like:** winning a night sprint clean, then losing a three-unit pursuit in the
  Calloway Tunnel and rolling into a safehouse with a cracked bumper and a full wallet.
- **Failure costs:** repair bills, fines, impound fees, rep, and in Simulation sometimes the car.
- **Key screens:** HUD (speed, damage silhouette, heat meter, radar/scanner), garage, mod tree,
  meet lobby, results + payout breakdown, impound.

### 2.2 The chase cop
*"I don't need to be faster. I need to be where they're going."*

- **Verbs:** patrol, spot, report, shadow, box in, PIT (when authorised), place spikes, hold a
  roadblock, follow the captain's orders or argue with them.
- **Success looks like:** shadowing a faster car at a safe distance for two minutes while the
  helicopter keeps eyes on it, then being one of three units that close the box at the Iron Bridge.
- **Failure costs:** a wrecked cruiser (self-wreck XP penalty), civilian collisions, a burnt risk
  budget that drops you to shadow duty, a suspect who escapes.
- **Key screens:** HUD with risk budget and orders feed, rank and unlocks, scanner.

### 2.3 The captain
*"Eight units, one helicopter, two spike kits. Make them count."*

- **Verbs:** watch the map, assign units, request roadblocks/spikes/helicopter, set priority targets,
  call off a pursuit that's too dangerous.
- **Success looks like:** predicting that a racer will run for the Kettle Street Drawbridge, timing
  it to the lift schedule and parking two units on the far side.
- **Failure costs:** wasted cooldowns, units disabled, a pursuit that ends in a pile-up on Lantern
  Way (heat on the department: fewer resources next shift, see §4.3).
- **Key screens:** captain map (2D roads, units, incidents, heat by district, last-known positions
  with staleness), command radial, scanner.

When no human captain is present, the dispatcher AI plays the captain with the same command set.

---

## 3. Core loops

### 3.1 The clock
`wp_world` default **(code)**: 1 real minute = 10 game minutes, so a game day is 2.4 real hours.
Day phases **(code, contracts)**: Dawn 05–07, Day 07–18, Dusk 18–20, Night 20–05.
Night therefore lasts 54 real minutes and Day 66; a player who logs in for 20 minutes sees about
three and a half game hours.

### 3.2 Moment to moment (seconds)
Drive → read the road (traffic, signals, surface, weather) → commit to a gap or a line → contact or
no contact → the car changes (dent, pull, lost part, overheating) → adapt. In a pursuit, add: check
radar/scanner → choose to run, hide or stop.

### 3.3 Session (minutes)
Garage → cruise → meet → race → heat → escape or capture → garage. The garage decision (repair all,
repair some, drive damaged, scrap) closes every session and is where money pressure is felt.

### 3.4 Meta (days and weeks)
Earn → upgrade car or durability → move up a class bracket → unlock licences and higher-stakes
events → join a crew → hold territory. Cops: rank up → unlock tools → become captain-eligible.

### 3.5 A typical 20-minute night session (racer)
Real time from login. Game time assumes login at 22:00. Car: a Compact (Vireo XS class).

| Real | Game | Beat | What happens | Systems |
|---|---|---|---|---|
| 0:00 | 22:00 | **Garage** | Car condition from last session, outstanding bill, choose drive-damaged or repair (≈ 1 min). | 08 garage, 03 repair estimate |
| 1:00 | 22:10 | **Cruise** | Drive from the safehouse to tonight's meet (Starlite Drive-In). Light traffic, a patrol passes; radar chirps. Legal driving = no heat. | 05 traffic, 06 patrol, radar |
| 3:30 | 22:35 | **Meet** | Lobby window 60–90 s: 4–8 racers, pick the event (a 6 km sprint), pay the buy-in. | 15 scheduler, 07 lobby, 08 buy-in |
| 5:00 | 22:50 | **Race 1** | 6 km sprint through Midtown and Lantern Row, ≈ 4 min. Speeding and a red light get witnessed: heat climbs to L1–L2 by the finish. | 02, 03, 05 violations, 06 heat |
| 9:00 | 23:30 | **Cool-down choice** | Collect the purse. Either drive legal for 1–2 min to let heat decay, or accept an outrun challenge while still warm. | 06 decay, 08 payout |
| 10:30 | 23:45 | **Race 2 / Outrun** | 1v1 on the Loop, ≈ 3.5 min. | 15 Outrun |
| 14:00 | 00:20 | **Pursuit** | A unit with line of sight escalates to L3; the dispatcher requests the helicopter (night: 120 s launch, 25 % chance unavailable). 3–4 min of shadowing; the racer breaks contact in the Calloway Tunnel and reaches a safehouse. | 06 pursuit, helo, escape rule |
| 18:00 | 01:00 | **Safehouse → garage** | Pursuit ends after 10 s in the safehouse with no unit within 150 m. Session summary: purses, bounty gained, damage quote. | 06, 08 |
| 20:00 | 01:20 | **Out** | Quit or continue. | |

Expected result: 2 races, ≈ 1 pursuit; net ≈ +2 500 to +3 500 before repairs (matches the economy
target "mid-skill nets ≈ 3 000 / night before repairs", §5).

### 3.6 A daytime sanctioned event
Legal, closed-road racing. Lower pay per km, no heat, licence progress, and the clean-race bonus
matters most here.

| Real | Game | Beat |
|---|---|---|
| 0:00 | 10:00 | Event appears on the calendar (15) 1 game hour ahead. Registration at the start (e.g. Founders Plaza). Entry fee, class bracket check. |
| 2:00 | 10:20 | Closure activates: traffic suppressed on the route, cones and marshals placed (05 `RouteClosure`). |
| 3:00 | 10:30 | Qualifying: one TimeAttack lap sets the grid. |
| 6:00 | 11:00 | Race: e.g. Harbor Grand Prix, 5 laps × 3.8 km ≈ 12–14 min. Contact is penalised by your own repair bill and loses the clean bonus. |
| 20:00 | 13:20 | Results, purse (day multiplier 0.75 **(code)**), licence credit, rep. |

Leaving the closure corridor is a DQ and a normal traffic violation. Nothing that happens inside
the closure creates heat.

### 3.7 An illegal day race (rare)
The spectacle event: very high purse, daytime police presence at maximum, and a pursuit is the
expected outcome, not a risk.

| Real | Game | Beat |
|---|---|---|
| −5:00 | | The scheduler (15) flags one illegal day race in a game day at most; racers get a scanner/phone tip 3–5 real minutes before. |
| 0:00 | 13:00 | Grid forms on a busy street (R02 Lantern Run, R03, R11 or R14). Starting a race in front of witnesses at noon is an immediate L2 for everyone on the grid. |
| 1:00 | 13:10 | Race, 4–6 min through peak daytime traffic. Units are already converging (day reinforcement 20–40 s). |
| 6:00 | 14:00 | Finish line is where the pursuit really starts: 3–8 min to escape. Expect about half the field to be caught. |

Purse: the race type's base × day 0.75 × wanted up to 1.45 **(code)** × an illegal-day multiplier
owned by 15 (target: the winner earns about 3 night wins; a capture still costs more than the win
pays at L4–L5, see §5).

### 3.8 Cop session (20 minutes)
Spawn at a precinct → patrol an assigned district (clean minutes earn XP) → respond to an incident
or a violation → pursuit under the captain's direction → capture, assist or disengage → back on
patrol. A good cop shift is 1–2 pursuits and 15+ clean minutes.

---

## 4. Progression

### 4.1 Racer: cars and class brackets
Class index brackets **(code, `wp_economy::class_index`)**: D < 300 ≤ C < 450 ≤ B < 600 ≤ A < 750
≤ S < 900 ≤ X. Lobbies and events bracket by class index, not by car.

| Stage | Typical car | Price **(code)** | Target time to reach |
|---|---|---|---|
| Start | Junkyard beater (floor car) | 1 500 (always affordable) | session 1 |
| First real car | Vireo XS (Compact) | dealer price clamped to the Compact band, ≥ 9 000 | after 2–3 good nights |
| Mid | Kestrel TS / Shiro Turbo / Sperber 32 / Lindwurm 4T (Sport), Ironside 440 (Muscle) | 22 000 – 38 000 | 8–12 nights |
| Upper | Hakone GT, Rampart 5.0 | 42 000 – 48 000 | 15–25 nights |
| Top | Lucciola 350 (Exotic) | 120 000 | 40+ nights; a goal, not a requirement |

The Hauler 1500 (Truck) is a sideways step: slow, nearly unbreakable (durability 5), the best
Delivery and Cannonball car, and the cheapest way to survive police contact.

### 4.2 Racer: rep, licences, crews
- **Rep** **(code)**: placings pay 120/70/40/15 rep, DNF −20, clean race +25; night ×1.4, day ×0.8.
  Rep gates events and pink slips (500 rep minimum **(code)**).
- **Licence tiers** (15): unlocked by counts of *clean* finishes, not wins. Proposed: Street
  (start) → Club (10 clean finishes) → Pro (30 clean, 5 wins) → Underground (60 clean, 15 wins, rep
  ≥ 2 000). Licences open higher-stakes events and the illegal day race invitations.
- **Crews** (15): shared rep, crew bank cut, territory (districts held by wins; a controlled
  district gives a small heat-decay bonus and a safehouse). Territory decays weekly so crews must
  keep racing.

### 4.3 Police: cop rank
Rank thresholds **(code, `consequences.toml`)**: 500 / 1 500 / 3 500 / 7 000 / 12 000 / 20 000 XP.
XP **(code)**: capture 20–500 by wanted level, assist 10–200, clean patrol minute +2, civilian
collision −40, self-wreck −150, risk-budget overspend −60.

| Rank | XP | Unlocks (design target for 06/08) |
|---|---|---|
| 0 Probationary | 0 | Patrol cruiser (Sentinel Cruiser), shadow, box-in assist |
| 1 Officer | 500 | Lead box-in, rolling block, radio requests |
| 2 Senior Officer | 1 500 | Spike strip kit, Sentinel Wolfhound interceptor |
| 3 Corporal | 3 500 | PIT authorisation (within doctrine limits), roadblock lead |
| 4 Sergeant | 7 000 | Sentinel Bastion SUV, can request the helicopter |
| 5 Lieutenant | 12 000 | Captain-eligible (lobby may assign) |
| 6 Captain | 20 000 | Captain priority in matchmaking; captain cosmetics |

At a rate of about 300–500 XP per good shift, rank 2 takes 3–5 shifts and captain eligibility
about 25–40. Captures dominate XP at high wanted levels, but one self-wreck erases a capture at
L2, so careful cops rank faster than aggressive ones.

**Department standing (proposal, see backlog):** captain shifts that end with many civilian
injuries reduce next shift's helicopter and roadblock budget for that instance.

### 4.4 Pacing targets
- A new racer should win something in their first session (the free street meet is tuned for that).
- Session-to-session progress should be visible: every session ends with a number going up (cash,
  rep, licence progress or cop XP), even a bad one.
- A player who loses a car in Simulation should be back in a usable car in one session (economy
  floor) and back to their previous class in about a week of play.

---

## 5. Economy intent

The economy is `wp_economy`'s, tuned by workstream 08; this section states the intent those numbers
serve so other teams design toward the same feel. Source: `crates/wp_economy/README.md` and
`data/economy/*.toml`.

### 5.1 Anchor numbers **(code)**
| What | Value |
|---|---|
| Starting cash | 4 000 |
| Floor car (junkyard beater) | 1 500, or all your cash if less; never creates debt |
| Compact 9 km night sprint win | ≈ 2 800 (230/km × 9 km × night 1.35) |
| Mid-skill net per night, before repairs | ≈ 3 000 |
| Placing shares | 100 / 50 / 25 / 10 % of the winner purse; clean race +25 % |
| Night / Dawn / Dusk / Day purse | ×1.35 / ×1.1 / ×1.0 / ×0.75 |
| Simulation purse | ×1.2 |
| Racing while wanted | ×1.05 (L1) … ×1.45 (L5) plus bounty |
| Fines | 600 (L1) … 9 000 (L5), +15 % per prior capture; Arcade ×0.6, Simulation ×1.25 |
| Bail | 50 % of bounty |
| Pound | auction after 10 days |
| Repairs | Arcade costs half of Simulation |

### 5.2 Feel targets **(code)**
- A bumper scrape costs less than one buy-in.
- A medium front hit costs about one race win in Simulation, half in Arcade.
- A full wreck costs about 2× the car's value in Simulation (never below 60 %).
- An L5 bust costs about 2 nights in Arcade and about 4 in Simulation.
- 20-night career (scripted mid-skill): Arcade median ends at 2.5× starting net worth, nobody in
  debt; Simulation median ends at about 0.9× (marginal) with some careers in debt.

### 5.3 Design rules
- **You pay for what you break.** Damage is billed per region and component.
- **Debt is a slow grind, not a game over.** Only penalties create debt; in debt you can only enter
  free street meets, whose winnings pay it down.
- **Crooked vs legit shops** are a risk choice: crooked shops cost more and never report; legit
  shops are cheaper and report wanted cars (L2+). Map layout (`CITY.md` §5.2) puts legit shops
  north of the river near precincts and crooked shops south.
- **Insurance** does not cover damage taken while wanted.
- **No currency sold for real money** (§11).

### 5.4 Escape rewards (proposal to 06/08)
Escapes already add bounty and rep by wanted level × day-phase multiplier **(code)**. Proposal:
a small rep bonus for escapes into "committed" safehouses (one-way approaches: SH02 Pier 9, SH06
Knob Observatory, SH03 Lantern Garage) to reward risk over the nearest bolt-hole.

---

## 6. Difficulty and assists

### 6.1 Three independent axes
| Axis | Options | Owner | Affects |
|---|---|---|---|
| **Damage mode** | Arcade, Simulation | 03 (`data/modes/*.toml`), 08 | Damage per impulse, total-loss thresholds, repair cost, fines, seizure |
| **Assists** | ABS, TCS, ESC, steering help, throttle filter (each toggle) | 02 `AssistProfile` | Only how inputs become forces |
| **Lobby / time of day** | Day vs night, wanted level you choose to carry, class bracket | 06, 15 | How dangerous the world is |

These never merge into one "difficulty" slider in the sim. The UI offers three **presets** that
set the first two together, and the player can change any toggle afterwards:

| Preset | Damage mode | Assists |
|---|---|---|
| Rookie | Arcade | all on, keyboard throttle filter on |
| Street (default) | Arcade | ABS + TCS on, ESC low, steering help off |
| Sim | Simulation | all off |

### 6.2 Rules
- **Assists never add grip or power.** They only manage what the tires already have.
- **Opponent AI difficulty comes from car and driver skill parameters only** (17's
  `DriverProfile`); never from speed boosts, grip boosts or catch-up logic.
- **Police difficulty comes from tables** (presence, response times, helicopter availability) keyed
  by day phase and wanted level. A player turns police difficulty up by racing at noon or carrying
  heat, not by a menu.
- **Keyboard gets filtered input by default;** gamepad and wheel get raw input with optional filtering.
- Online lobbies are split by damage mode; assists are free choice everywhere (shown as HUD
  badges and on leaderboards).

### 6.3 Arcade vs Simulation in one line each **(code, `CLAUDE.md`)**
- **Arcade:** punishing but survivable; one hard hit degrades, several end the car.
- **Simulation:** real-world-ish; one hard hit at speed is a total loss.

---

## 7. Modes

| Mode | Players | Description |
|---|---|---|
| **Free roam (city)** | 1–24 per instance | The persistent city with traffic and police. All other activities start from here. |
| **Night meets** | racers | Scheduled meets at POIs; Sprint, Circuit, Drag, Touge, Outrun, Checkpoint, Cannonball, Delivery. |
| **Sanctioned events** | racers | Daytime closed-road circuits and TimeAttack (`CITY.md` §7). |
| **Illegal day race** | racers vs police | Rare calendar event (§3.7). |
| **Pursuit (asymmetric)** | racers vs player cops + captain | Not a separate mode: whenever a racer is wanted, player cops in the instance can join. |
| **Captain** | 1 per instance | Map and command role; dispatcher AI fills it if empty. |
| **Time attack / leaderboards** | solo | Any route R01–R15, ghosts from replays. |
| **Offline** | solo | Same city, AI racers (17) and AI police; same economy, separate save. |
| **Test lot** | solo | The garage test lot: try mods, repeatable crash tests, replay. |

Instance target: 24 player-controlled vehicles (the perf budget is sized for 24 cars), suggested
role split 14 racers / 9 cops / 1 captain, matched by 18. Civilian traffic is separate (up to 400
agents per instance).

---

## 8. Session structure (online)

- **Join:** the player picks role (racer/cop), damage mode and class bracket; matchmaking (18)
  places them in a city instance. Spawn at your safehouse (racer) or precinct (cop).
- **Day cycle:** the instance clock runs continuously (2.4 h real per game day). Joining players
  land at whatever time it is; the calendar shows upcoming meets and events.
- **Persistence:** cash, rep, bounty, garage, damage state of every owned car, impound, cop rank,
  crew data persist (08, 18). Heat does not persist between sessions; bounty does.
- **Leaving:** logging out in a safehouse or garage is clean. Disconnecting while wanted at L2+
  gives a 60 s reconnect grace (18's reconnect tokens); after that it counts as a capture. This
  stops escape-by-quitting.
- **End of game day:** daily pound fees, insurance premiums, territory and leaderboard ticks
  (`end_of_day` in 08).

---

## 9. Onboarding (first 30 minutes)

Goal: teach driving, damage and the money loop without a tutorial wall. Every step happens in the
real city.

1. **Cold open (2 min):** night, the player drives a borrowed Rampart 5.0 in a scripted-free but
   staged chase on the Loop: one patrol, helicopter overhead, Calloway Tunnel ahead. The player
   learns that breaking line of sight works. The car is not theirs; whatever happens to it is free.
2. **Junkyard (1 min):** wake up with 4 000 and the junkyard beater offer. Buy it.
3. **Test lot (3 min):** drive, brake, a 30 km/h barrier tap. The damage silhouette and the repair
   quote appear. Choose "drive damaged" or repair: both are valid.
4. **First meet (5 min):** Mercer Meet (R10): free entry, short, tuned so a careful beater can
   place. Payout screen explains purse, clean bonus, rep.
5. **First heat (5 min):** a scripted-free trigger: the next run passes a stationary patrol. The
   radar and heat meter tutorial fires on the first real violation. Escape to the nearest
   safehouse (the HUD highlights the two nearest).
6. **First garage visit:** repair bill, a legit vs crooked shop choice explained in one line.
7. **Choice of path:** at the end, the game offers a cop shift (with a probationary cruiser) as an
   equal alternative. Role choice is never locked.

Assist preset defaults to Street; the game suggests Rookie if the player crashes three times in the
first five minutes, and never changes it silently.

---

## 10. Failure states and recovery

| Failure | Trigger (source) | Cost | Recovery |
|---|---|---|---|
| DNF | total loss, leaving the corridor, capture during a race (15) | no purse, −20 rep | next race |
| Total loss | damage mode thresholds (03) | Arcade: towed, big repair bill. Simulation: car destroyed if captured; otherwise scrap or very expensive repair | floor car, insurance |
| Capture | stopped < 2 m/s for 4 s with a unit within 15 m, or total loss while wanted (06) | fine + bail + rep loss + impound; Sim: 30 % seizure at L4–L5 (08) | pay at the pound within 10 days |
| Impound | capture (08) | daily fee | retrieve or lose at auction (10 % back) |
| Debt | fines/bail exceeding cash (08) | voluntary spending blocked | free street meets |
| Pink-slip loss | lose a consented pink-slip race (08) | the car, with mods and damage | — |
| Cop self-wreck | unit total loss without racer contact (06) | −150 XP | respawn at precinct after a delay |
| Cop risk budget spent | lateral g, near misses, collisions (06) | demoted to shadow mode for this pursuit | next pursuit |
| Captain overreach | civilian injuries during pursuits | department standing (proposal) | next shift |

**Never a dead end:** the floor car and free meets guarantee every player can always race again.

---

## 11. Deliberately not in the game

| Not in the game | Why |
|---|---|
| **Invincible cars** (NFS-style: full health after a wall hit at 200 km/h) | Breaks pillar 1 and 2; the damage model is the game. |
| **Invincible or super-fast police** | Police that out-drive physics wreck themselves here, by design. They win by tactics. |
| **Rubber-banding** (AI or police speed-up when behind, slow-down when ahead) | Breaks pillar 5. AI skill and car choice are the only levers. |
| **Pay-to-win** (buying cars, parts, cash, repairs or heat clearance with real money) | Breaks pillar 5. Cosmetics only, if anything is ever sold. |
| **Nitrous in Simulation** | Arcade only **(code)**; with engine damage risk. |
| **Weapons**, pedestrians to hit, killing | Tone: dangerous driving, not violence. The helicopter is eyes, not a weapon. Pedestrians are not simulated. |
| **Instant heat clearing** (paint shop = clean slate) | Heat decays with time and broken contact; informants cost money and only reduce it. |
| **Escape by quitting** | Disconnect while wanted counts as a capture (§8). |
| **Licensed cars, brands or real places** | `CLAUDE.md` rule 9; Port Vesper is fictional. |
| **Full soft-body (BeamNG-style) or cosmetic-only damage** | Rigid chassis + lattice is the chosen model (design overview §1). |
| **Scripted crash cinematics / takedown cams that pause physics** | Replays and photo mode show the real simulation instead. |
| **Loot boxes, timers to skip, energy systems** | Respect for players' time. |

---

## 12. Dependencies and open questions

| Topic | Needs | From |
|---|---|---|
| District identity everywhere (crews, heat maps, labour) | `DistrictId` / zone names | lead, 05 (`CITY.md` C2) |
| Garage legit/crooked on the map | POI shop kind | 05, 08 (`CITY.md` C3) |
| Drawbridge, trains, port carriers | dynamic world entities | 05 (`CITY.md` C4) |
| Helicopter LOS in tunnels / parking structures | covered-lane flag + LOS query | 05, 06 (`CITY.md` C5) |
| Licence tiers and illegal-day multipliers | numbers in `data/events/` | 15 |
| Escape rewards per safehouse | rep bonus table | 06, 08 (§5.4) |
| Department standing for captains | budget feedback | 06, 08 (backlog) |
| Junkyard beater car file | `junkyard_beater.json` | 09 |
| Disconnect-while-wanted rule | reconnect grace | 07, 18 |
