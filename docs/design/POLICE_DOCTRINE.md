# PVPD Pursuit Doctrine — in-fiction policy and radio guide

**Owner:** workstream 19 (design docs). **Consumers:** 06 police (AI tables in `data/police/*.toml`,
dispatch, captain), 13 audio (scanner lines), 16 UI (captain map, scanner feed, HUD wanted meter),
08 economy (cop XP), writers.

The Port Vesper Police Department (PVPD) is fictional. This document is its pursuit policy as an
officer would read it, written so that **every rule maps to a number or behaviour in `wp_police`**.
If the AI does something this document forbids, one of them is wrong: file it in
`docs/integration/design-docs.md` and the owner of the AI tables (06) decides which.

City names, districts, crossings and route ids come from `docs/design/CITY.md`.

---

## 1. How to use this document

- **AI/tables (06):** §3–§5 are the behaviour spec in plain words. Numbers marked *(06 table)* live
  in `data/police/*.toml` keyed by `DayPhase`; the values here are design intent, the tables are
  the truth once they exist.
- **Captain UI (16) and net (07):** §6 is the command vocabulary; use these words on buttons.
- **Audio (13):** §8 is the call-sign and token scheme, §9 the 150 lines, §10 the selection rules.
- **Writers:** cops in Port Vesper are tired, careful professionals with bad cars and good radios.
  They are not villains and they are not invincible.

---

## 2. Doctrine principles

PVPD General Order 41 ("Vehicle Pursuits"), condensed. Each principle is a mechanic.

| # | Principle (in-fiction) | Mechanic in `wp_police` |
|---|---|---|
| P1 | **Contain, don't race.** A pursuit is won by the radio, not the gas pedal. | Pursuit speed cap = `min(target_speed × 1.10, pursuit_cap(phase))`; `Intercept` routes units to the racer's predicted position 10–20 s ahead. |
| P2 | **Slow and calculated.** Shadow at distance until the situation favours you. | Default tactic `Shadow` (25–40 m behind, mirror lane). Aggressive tactics need conditions (§3.3). |
| P3 | **Nobody is invincible.** Your car breaks like theirs. A wrecked unit helps nobody. | Cops use the same damage model; each unit has a **risk budget** (time above 0.7 g lateral, near-misses, collisions). Spent budget → back to `Shadow`. |
| P4 | **Attrition over aggression.** Spikes, blocks and fatigue end pursuits; rams don't. | Tools (`SpikeAt`, `RoadblockAt`, `RollingBlock`) placed ahead on the predicted path. Contact tactics only below speed limits (§3.3). |
| P5 | **The helicopter is eyes, not a weapon.** | `Helicopter` is kinematic, gives LOS to all units, never collides or blocks. |
| P6 | **Civilians first.** If the chase is more dangerous than the suspect, call it off. | Supervisor termination (§4.3) on civilian-risk triggers. |
| P7 | **Day is ours, night is theirs.** Full toolset by day; thin coverage at night. | All counts, response times and helicopter availability keyed by `DayPhase` *(06 table)*. Night bounties pay more (08). |
| P8 | **Witnesses make the case.** We act on what we see; reports only build a picture. | A violation seen by a unit or camera counts 100 % to heat; unseen ("reported") counts 20 %. |

---

## 3. Wanted levels and what they authorise

### 3.1 Heat and levels

Each racer has `heat` 0–100 mapped to `WantedLevel` (`None`, `L1`–`L5`). Proposed thresholds
*(06 table)*: L1 ≥ 10, L2 ≥ 25, L3 ≥ 45, L4 ≥ 65, L5 ≥ 85. Heat decays only while no unit has line
of sight; decay is faster at night and in Suburb / Industrial districts.

| Level | In-fiction name | Typical trigger (witnessed) | Example |
|---|---|---|---|
| None | — | nothing seen | Driving at the limit. |
| L1 | Traffic violation | Speeding > 10 mph over for > 1 s, one red light | A unit lights you up for a stop. |
| L2 | Failure to yield | Ignoring a stop for > 5 s, repeated speeding, wrong way briefly | First pursuit. |
| L3 | Reckless evading | Near-misses, red lights at speed, contact with a civilian | Backup and intercepts. |
| L4 | Felony evading | Disabling a unit, breaching a roadblock, hit-and-run | Spikes, roadblocks, helicopter. |
| L5 | Priority target | Several disabled units, injuries, racing while at L4 | Everything available, captain priority. |

Reports (unwitnessed violations, 20 %) can raise heat to L1 on their own but never start a pursuit:
dispatch puts out a description ("be on the lookout", BOLO) and the next unit with LOS starts it.

### 3.2 Authorisation table

Max units = ground units allowed to actively pursue at once (others hold chokepoints or intercept).
Response time = reinforcement arrival after request. Values are design intent *(06 table)*.

| Level | Tactics authorised | Max pursuing units | Tools | Day | Night |
|---|---|---|---|---|---|
| L1 | Traffic stop, `Shadow` | 1 | — | response 20–40 s | response 60–150 s |
| L2 | + `BoxIn`, `Intercept` | 2 | — | as above | as above |
| L3 | + `RollingBlock`, `PIT` (conditions §3.3) | 3 | Spikes (on request) | helicopter on request, launch 45 s | helicopter on request, launch 120 s, 25 % unavailable |
| L4 | + `RoadblockAt` | 4 | Spikes, roadblocks, helicopter | helicopter auto-launched | helicopter auto-requested (same night rules) |
| L5 | all | 6 | all; captain priority target | all reinforcements prioritised | slowest reinforcements still apply; bounty × night multiplier |

Patrol units on shift per district and phase are in `CITY.md` §5.3 (e.g. Lantern Row 8/6/3/2).
At night a whole district may have one car; that car will wait for help rather than chase alone.

### 3.3 Conditions on each tactic

| Tactic | Conditions (all must hold) |
|---|---|
| `Shadow` | Always allowed. Hold 25–40 m behind, mirror the lane, report position. |
| `BoxIn` | 2–3 units (front-left, front-right, rear), target below **60 km/h** (37 mph). |
| `PIT` | Target below **55 km/h** (34 mph) in Simulation, **70 km/h** (43 mph) in Arcade; on a straight; no civilians within 30 m; executing unit has risk budget left. Never on bridges, the drawbridge, or Bluff Road. |
| `RollingBlock` | Two or more units ahead of the target in the same direction, slowing progressively; not on single-lane roads (Iron Bridge, Bluff Road, Hollins Drive). |
| `Intercept` | Lane-legal route to the predicted position 10–20 s ahead; never drive against traffic to get there. |
| `SpikeAt` | L3+; placed ahead on the predicted path; segment sight distance **≥ 120 m**; not where civilians are queued; deploying unit clears the lane after. |
| `RoadblockAt` | L4+; 2–3 parked units + barriers ≥ **300 m** ahead of the target on the predicted path; sight distance ≥ 120 m; never round a blind corner; never on Hollins Drive. **Highways:** static blocks only at on/off ramps, never across Loop 9 or Route 40 mainline at L4–L5 (closing speeds too high) — there the doctrine is spikes on ramps + helicopter. *(Open for 06: the prompt says "never on highways above a level threshold"; this is our reading.)* |
| `HeloTrack` | L3+ (on request) / L4+ (automatic). Spotlight at night. Loses LOS in covered roads (Calloway Tunnel, Founders Cut, Lantern Underpass, parking structures). |

---

## 4. Risk, termination, escape and capture

### 4.1 Unit risk budget
Every unit starts a pursuit with a risk budget *(06 table)*. It is spent by time above 0.7 g
lateral, near-misses with civilians and any collision. When it is spent the unit **drops back to
`Shadow`** and is no longer eligible for PIT, box-in or rolling blocks. In-fiction: "back it off,
you're done driving point."

### 4.2 Unit condition
- Below **40 % health** → `Disengage`: the unit pulls over and reports itself out of service.
- A unit whose `HandlingEffects` show a bent wheel, lost wheel, punctured tire or overheating
  pulls over regardless of health. Cops obey their own damage.
- A unit that wrecks itself without racer contact is a **self-wreck** (costs a cop player XP).

### 4.3 Supervisor termination
The supervisor (captain or dispatcher-as-captain) calls the pursuit off when any of these hold
*(06 table, per phase)*:
- two or more civilian collisions in the pursuit within 60 s;
- target in a school zone during school hours (Mercer St) above 2× the limit;
- heavy rain or fog with pursuit speed above the weather cap (14's hook lowers caps);
- three units disabled with no tools left to deploy and no helicopter.

After termination heat stops rising from that pursuit but does not reset: the BOLO stands and
decay starts only when no unit has LOS.

### 4.4 Escape, safehouse, capture (end states)
| Outcome | Rule | In-fiction |
|---|---|---|
| **Escape** | No unit (or helicopter) has LOS for `escape_time(level, phase)` *(06 table; proposal: L1 10/8 s, L2 20/15 s, L3 35/25 s, L4 50/40 s, L5 70/55 s, day/night)*. | "All units, suspect lost, resume patrol." |
| **Safehouse** | Racer inside a safehouse POI for 10 s with no unit within 150 m. | "Lost him somewhere in {district}. Mark it." (Dispatch notes the area; using one safehouse often makes it known — design hook for heat decay, not v1.) |
| **Capture** | Racer below 2 m/s for 4 s with a unit within 15 m, or `TotalLoss` while wanted. | "Suspect in custody." → fine, impound, bail (08). |

---

## 5. Port Vesper chokepoint doctrine

The river has six crossings (`CITY.md` §3). PVPD's whole plan for cross-town pursuits is to own
them.

| Place | Doctrine |
|---|---|
| **Meridian Bridge** | Default block point for north–south pursuits through the centre. At L4 a roadblock goes at the south abutment (sight distance is good from both sides). |
| **Iron Bridge** | 1+1 lanes, steel grate. One parked unit closes it. Never PIT or roll on the deck; block at the ends. Wet: no pursuit across it at all. |
| **Kettle Street Drawbridge** | When the deck is lifting, it **is** the roadblock: dispatch tells units to hold the approaches, not chase. Units never attempt the gap. |
| **Westline Viaduct / Loop 9** | No static blocks on the mainline at L4–L5. Spikes on the ramps, helicopter on the viaduct (best LOS in the city). |
| **Westline Low Bridge** | Coordinate with train schedule: a passing train closes the crossing for both sides. Units wait. |
| **Calloway Tunnel** | Helicopter loses LOS inside; scanner static. Doctrine: before the target enters, units are sent to **both portals** and the next exits (Harbor Blvd, Kettle St). Nobody chases inside at speed: concrete walls. |
| **Founders Cut / Lantern Underpass** | Same LOS loss, short; helicopter reacquires at the far portal. |
| **Hollins Drive** | **No spikes, no roadblocks, no PIT** (blind crests, sight distance < 120 m). Units hold the bottom (Loop interchange) and the Knob; helicopter tracks. |
| **Bluff Road** | No contact tactics (cliff). Shadow and hold Harbor Blvd at the bottom. |
| **Port stacks (Gantry Row, Terminal)** | LOS breaks constantly. Units hold the terminal gates and the Port Connector; helicopter is the only eyes inside. Straddle carriers do not yield to anyone. |
| **Rail crossings** | Units never race a train. A closed crossing ends a ground chase on that road; dispatch reroutes intercepts. |
| **Lantern Row** | Most cameras: every violation is witnessed. Many civilians: termination triggers come early. |

---

## 6. The captain and the command vocabulary

The captain (a human player, or the dispatcher AI when none is present) commands the pursuit from
the map. Orders to **player** chase units are suggestions with a marker on their HUD; orders to AI
units are executed. These words map to `CaptainCommand` (06) and should be the button labels (16).

| Command | Meaning | Limits *(06 table)* |
|---|---|---|
| **Assign** {unit} → {task} | Patrol, Respond, Pursue, Intercept, Hold {crossing} | unit must be in service |
| **Request roadblock** at {segment} | Places 2–3 units + barriers | L4+, cooldown, sight ≥ 120 m, ≥ 300 m ahead |
| **Request spikes** at {segment} | One unit deploys a strip | L3+, limited strips per pursuit |
| **Request air support** | Launch Skywatch | per-phase launch time and availability |
| **Set priority target** {suspect} | All free units focus one racer | one at a time |
| **Call off** | Terminate pursuit (supervisor termination) | always allowed |

---

## 7. Cop-player conduct and rank

Cop players are held to the same order. `wp_economy` scores them:
- **Earn** XP for captures, assists, and clean minutes in pursuit (no contact with civilians).
- **Lose** XP for civilian collisions, self-wrecks and overspending the risk budget (driving point
  after being told to back off).
- Capture reward = base by level + 25 % of the racer's bounty; assists get 30 % of that.
- Rank unlocks the toolset in order: spikes → roadblocks → air support requests → captain
  eligibility (GDD progression).

The intended feel: a good cop is a patient one. The leaderboard rewards captures per civilian
incident, not top speed.

---

## 8. Radio scheme

### 8.1 Call signs
| Who | Pattern | Examples |
|---|---|---|
| Dispatcher | `Vesper Dispatch` (short: "Dispatch") | Vesper Dispatch |
| Patrol car (Cruiser class) | `<Precinct>-<2 digits>` | Central-12, Harbor-07, Loop-21 |
| Interceptor (pursuit sedan) | `<Precinct> Lima-<n>` | Central Lima-2, Loop Lima-1 |
| SUV (roadblock / heavy) | `<Precinct> Bravo-<n>` | South Bravo-3 |
| Supervisor on the ground | `<Precinct> Sergeant <n>` | West Sergeant 1 |
| Captain (human or AI) | `Command` | Command |
| Helicopter | `Skywatch One` | Skywatch One |

Precinct prefixes: **Central** (Lantern Row), **Harbor** (Old Harbor), **North** (Northfield),
**South** (Dunmore), **West** (Westline/Larkspur), **Loop** (Highway Patrol Post 9). Number blocks:
01–19 day shift, 20–39 evening, 40–59 night. Player cops get a call sign from the same pool.

### 8.2 Describing people, places, cars
- **Suspect vehicle:** colour + body style/class + partial plate: "a red coupe, partial plate
  Kilo-Seven-Four". Never a brand or model name. Classes are spoken as body styles: compact →
  "hatchback", sport → "coupe" or "sport sedan", muscle → "muscle car", exotic → "exotic", truck →
  "pickup".
- **Location:** street + cross street or landmark, or district: "Lantern Way at Meridian",
  "the Iron Bridge", "Hollins Drive near the Knob". Route ids (R01–R15) are never spoken; writers
  use the route's place names.
- **Direction:** compass words ("northbound on Kettle").
- **Speed:** mph ("eighty-plus").
- **Phonetic alphabet:** standard NATO letters for plates and unit names.

### 8.3 Codes (plain language first)
PVPD uses plain language. Only these codes are spoken:

| Code | Meaning |
|---|---|
| Code 2 | Respond, no lights or siren |
| Code 3 | Respond with lights and siren |
| Code 4 | No further help needed / situation under control |
| 10-4 | Acknowledged |
| 10-20 | Location ("what's your 20?") |
| 10-50 | Traffic collision |
| 10-80 | Pursuit in progress |
| 10-97 | Arrived on scene |
| 10-7 | Out of service |
| BOLO | Be on the lookout |

### 8.4 Template tokens (for 06 and 13)
Radio events are `PoliceEvent::Radio { unit, text }`. `text` is a template from §9 with tokens
filled by 06 at emit time:

| Token | Filled with |
|---|---|
| `{unit}` / `{unit2}` | call sign(s) |
| `{dispatch}` | "Vesper Dispatch" |
| `{street}` / `{cross}` | current street / nearest cross street or landmark |
| `{district}` | district name (`CITY.md` §2) |
| `{crossing}` | one of the six river crossings |
| `{dir}` | northbound / southbound / eastbound / westbound |
| `{colour}` | suspect colour word |
| `{class}` | spoken body style (§8.2) |
| `{plate}` | 2–3 phonetic characters |
| `{speed_mph}` | spoken speed, rounded to 5 |
| `{level}` | wanted level spoken as "level three" |
| `{eta}` | spoken seconds/minutes |
| `{count}` | number of units |

### 8.5 Scanner priority tiers
- **P1** — pursuit-critical: pursuit start, tool placed, helicopter LOS change, crash, termination,
  capture. Interrupts lower lines.
- **P2** — tactical updates: positions, backup en route, intercept assignments.
- **P3** — ambience: patrol chatter, unrelated calls, end of shift. Dropped first under load.

---

## 9. Sample radio lines (150)

Format: `- **Lnnn** [Speaker · Pn] text`. Speakers: Dispatch, Unit, Skywatch, Command, Supervisor.

### 9.1 Patrol and idle chatter
- **L001** [Dispatch · P3] All units, routine patrol. {district} reporting quiet.
- **L002** [Unit · P3] {unit}, clear from the last call, back in service in {district}.
- **L003** [Dispatch · P3] {unit}, noise complaint, engines revving, {street} at {cross}. Code 2.
- **L004** [Unit · P3] {unit}, 10-4, en route Code 2.
- **L005** [Dispatch · P3] Any unit near the Starlite Drive-In, report of cars gathering. Code 2.
- **L006** [Unit · P3] {unit}, nothing here but tire marks. Clear.
- **L007** [Dispatch · P3] {unit}, what's your 20?
- **L008** [Unit · P3] {unit}, {street} {dir}, coming up on {cross}.
- **L009** [Dispatch · P3] Units in {district}, BOLO stands for a {colour} {class}, partial {plate}.
- **L010** [Unit · P3] {unit}, quiet night on the Loop. Too quiet.
- **L011** [Dispatch · P3] {unit}, alarm call, Gantry Row reefer warehouse. Probably the wind again.
- **L012** [Unit · P3] {unit}, parked at Meridian Bridge, watching traffic.

### 9.2 Traffic stop and violation spotted
- **L013** [Unit · P2] {unit}, traffic stop, {colour} {class}, {street} at {cross}.
- **L014** [Unit · P2] {unit}, I've got a {colour} {class} doing {speed_mph} on {street}.
- **L015** [Unit · P2] {unit}, red light runner, {street} at {cross}, {dir}.
- **L016** [Unit · P2] {unit}, wrong-way driver on {street}. Lighting him up.
- **L017** [Dispatch · P2] Camera hit, {colour} {class}, {speed_mph} in Lantern Row, {dir} on {street}.
- **L018** [Dispatch · P3] Caller reports street racing on {street}. No units on scene.
- **L019** [Unit · P2] {unit}, vehicle not stopping. Giving it a few seconds.
- **L020** [Unit · P2] {unit}, driver pulled over. Checking plate {plate}.
- **L021** [Dispatch · P2] {unit}, plate {plate} comes back to a junked car. Use caution.
- **L022** [Unit · P2] {unit}, school zone on Mercer, {colour} {class} doing {speed_mph}. Kids out.

### 9.3 Pursuit start
- **L023** [Unit · P1] {unit}, 10-80, {colour} {class} {dir} on {street}, failing to yield.
- **L024** [Dispatch · P1] All units, {unit} is in pursuit, {dir} {street} from {cross}. Clear the channel.
- **L025** [Unit · P1] {unit}, he's running. Plate {plate}, speeds {speed_mph}.
- **L026** [Dispatch · P1] Copy {unit}. Suspect is now {level}. Units in {district}, start moving.
- **L027** [Unit · P1] {unit}, in pursuit, I'm hanging back, keeping eyes on.
- **L028** [Dispatch · P1] All units, {colour} {class}, {level}, last seen {street} {dir}.
- **L029** [Unit · P1] {unit}, joining, I'm two blocks behind {unit2}.
- **L030** [Unit · P1] {unit}, multiple vehicles, looks like a race. I'm taking the lead car.

### 9.4 Pursuit updates by level
- **L031** [Unit · P2] {unit}, still {dir} on {street}, passing {cross}, {speed_mph}.
- **L032** [Unit · P2] {unit}, he's turning onto {street}, {dir}.
- **L033** [Dispatch · P2] Pursuit update, suspect at {level}. Units hold positions, no heroics.
- **L034** [Unit · P2] {unit}, he blew the light at {cross}. Civilians braking hard.
- **L035** [Dispatch · P1] Suspect is {level}. Spikes are authorized.
- **L036** [Dispatch · P1] Suspect is now {level}. Roadblocks authorized. Air support requested.
- **L037** [Dispatch · P1] {level}. Priority target. All available units.
- **L038** [Unit · P2] {unit}, he's heading for the river, probably {crossing}.
- **L039** [Unit · P2] {unit}, he's on the Loop {dir}, speeds over a hundred.
- **L040** [Unit · P2] {unit}, he's slowing. Might be hurt. Car's smoking.
- **L041** [Unit · P2] {unit}, suspect lost a bumper on {street}. Debris in the lane.
- **L042** [Unit · P2] {unit}, he's into {district}, side streets. Watch the alleys.

### 9.5 Backup and reinforcements
- **L043** [Unit · P2] {unit}, requesting backup, {street} {dir}.
- **L044** [Dispatch · P2] {unit2} responding Code 3, ETA {eta}.
- **L045** [Dispatch · P2] Nearest unit is {eta} out. It's night, {unit}. Hold what you've got.
- **L046** [Dispatch · P2] {count} units en route from {district}.
- **L047** [Unit · P2] {unit}, 10-97, coming in behind {unit2}.
- **L048** [Dispatch · P2] Sorry {unit}, nobody free in {district}. You're on your own a minute.
- **L049** [Unit · P2] {unit}, taking the parallel on {street}, I'll pace him.
- **L050** [Dispatch · P2] Day shift's thick, {unit}. You'll have company in {eta}.

### 9.6 Air support
- **L051** [Dispatch · P1] Skywatch One, you're requested, {district}.
- **L052** [Skywatch · P1] Skywatch One, lifting from Harbor pad, ETA {eta}.
- **L053** [Dispatch · P1] Negative on air, Skywatch is down for maintenance tonight.
- **L054** [Skywatch · P1] Skywatch One, I have the {colour} {class}, {dir} on {street}.
- **L055** [Skywatch · P1] Skywatch One, I'm on him. Ground units, back off and let me call it.
- **L056** [Skywatch · P2] Skywatch One, he's turning {dir} onto {street}, three cars ahead of him.
- **L057** [Skywatch · P1] Skywatch One, he's entering Calloway Tunnel. I lose him in there.
- **L058** [Skywatch · P1] Skywatch One, lost visual, he's under the plaza. Watching the far end of Founders Cut.
- **L059** [Skywatch · P1] Skywatch One, reacquired at the portal, {dir}.
- **L060** [Skywatch · P2] Skywatch One, spotlight on. Fog's thick over the bluffs.
- **L061** [Skywatch · P2] Skywatch One, he's in the container stacks. I'm the only eyes in there.
- **L062** [Skywatch · P1] Skywatch One, he's in the parking structure. No line of sight.
- **L063** [Skywatch · P2] Skywatch One, low on fuel, returning to pad in {eta}.

### 9.7 Spikes
- **L064** [Command · P1] {unit}, deploy spikes {street} at {cross}.
- **L065** [Unit · P1] {unit}, spikes down, {street}, lane two.
- **L066** [Unit · P1] {unit}, he hit the spikes! Front left's going.
- **L067** [Unit · P1] {unit}, he hit the spikes. Tires shredding, he's sparking.
- **L068** [Unit · P1] {unit}, he dodged the strip. Missed.
- **L069** [Unit · P2] {unit}, pulling the strip, civilians coming through.
- **L070** [Dispatch · P1] Do not spike on Hollins Drive. Sight lines are bad. Hold the bottom.

### 9.8 Roadblocks
- **L071** [Command · P1] Roadblock at {crossing}. {unit} and {unit2}, set it.
- **L072** [Unit · P1] {unit}, roadblock is up at {street} and {cross}.
- **L073** [Unit · P1] {unit}, he's coming at the block! Brace!
- **L074** [Unit · P1] {unit}, he went through. Two units damaged at the block.
- **L075** [Unit · P1] {unit}, he turned off before the block. Taking {street}.
- **L076** [Unit · P2] {unit}, block's cleared, reopening the road.
- **L077** [Dispatch · P1] Drawbridge is going up. Hold the approaches, that's our roadblock.
- **L078** [Dispatch · P1] Nobody jumps that bridge. Let the river do the work.

### 9.9 Box-in, PIT, rolling block, intercept
- **L079** [Unit · P1] {unit}, he's slowing in traffic. Box him. {unit2} take the left.
- **L080** [Unit · P1] {unit}, I'm front right. Squeeze.
- **L081** [Unit · P1] {unit}, box complete, he's stopped.
- **L082** [Unit · P1] {unit}, requesting PIT, straight road, no civilians.
- **L083** [Command · P1] PIT authorized, {unit}. Clean and quick.
- **L084** [Command · P1] Negative on the PIT, too fast. Stay on him.
- **L085** [Command · P1] Negative, that's a bridge. No contact.
- **L086** [Unit · P1] {unit}, PIT executed. He's spinning.
- **L087** [Unit · P1] {unit}, PIT failed, he caught it.
- **L088** [Command · P2] {unit}, {unit2}, get ahead and roll it down. Easy.
- **L089** [Unit · P2] {unit}, rolling block forming, {street} {dir}.
- **L090** [Dispatch · P2] {unit}, intercept at {street} and {cross}, he'll be there in twenty.
- **L091** [Unit · P2] {unit}, set at the intercept. I see him coming.
- **L092** [Unit · P2] {unit}, he went the other way. Re-routing.

### 9.10 Unit damage, disengaging, self-wreck
- **L093** [Unit · P1] {unit}, I'm hit, front end's gone. Pulling out.
- **L094** [Unit · P1] {unit}, steering's bent, she pulls hard right. I'm 10-7.
- **L095** [Unit · P1] {unit}, lost a wheel. Out of the pursuit.
- **L096** [Unit · P1] {unit}, temp gauge pegged, I'm cooking. Backing off.
- **L097** [Supervisor · P1] {unit}, back it off, you're done driving point.
- **L098** [Unit · P1] {unit}, I lost it on {street}. Into the wall. I'm okay, car's not.
- **L099** [Dispatch · P2] Copy, {unit} out of service. Tow requested to {street}.
- **L100** [Unit · P2] {unit}, tire's flat from his debris. Stopping.

### 9.11 Civilian crashes and incident scenes
- **L101** [Unit · P1] {unit}, 10-50, suspect clipped a civilian at {cross}.
- **L102** [Dispatch · P1] Units, collision at {street} and {cross}, civilians involved. Medical rolling.
- **L103** [Unit · P2] {unit}, I'm staying on scene, three cars tangled.
- **L104** [Dispatch · P2] {unit}, hold the scene on {street}, traffic's backing up.
- **L105** [Unit · P2] {unit}, scene's Code 4. Opening one lane.
- **L106** [Dispatch · P3] Wreck from earlier still blocking {street}. Avoid.

### 9.12 Lost visual and search
- **L107** [Unit · P1] {unit}, lost visual. Last seen {dir} on {street}.
- **L108** [Dispatch · P1] All units, suspect lost near {cross}. Start a grid.
- **L109** [Unit · P2] {unit}, checking alleys off {street}.
- **L110** [Unit · P2] {unit}, I've got the {crossing}. Nothing crossing.
- **L111** [Dispatch · P2] {unit}, check the lots in {district}. He's somewhere close.
- **L112** [Unit · P1] {unit}, reacquired! {colour} {class}, {street} {dir}.

### 9.13 Escape and safehouse
- **L113** [Dispatch · P1] All units, suspect lost. Terminate the 10-80, resume patrol.
- **L114** [Dispatch · P1] He's gone. BOLO stays up, {colour} {class}, partial {plate}.
- **L115** [Unit · P2] {unit}, lost him somewhere in {district}. Mark it.
- **L116** [Dispatch · P2] That's the third time he's vanished in {district}. Someone's hiding him.
- **L117** [Unit · P3] {unit}, he's good. I'll give him that.

### 9.14 Capture and total loss
- **L118** [Unit · P1] {unit}, he's stopped! Approaching.
- **L119** [Unit · P1] {unit}, suspect in custody. Code 4.
- **L120** [Dispatch · P1] Copy, suspect in custody on {street}. Good work, everybody.
- **L121** [Unit · P1] {unit}, suspect car is totaled. Driver's out and walking. Detaining.
- **L122** [Dispatch · P2] Tow to the pound for the {colour} {class}. Paperwork's on {unit}.
- **L123** [Unit · P2] {unit}, assist on the arrest at {cross}.

### 9.15 Supervisor termination
- **L124** [Supervisor · P1] All units, terminate the pursuit. Too many civilians.
- **L125** [Command · P1] Call it off. Lights off, let him go. We'll find him later.
- **L126** [Supervisor · P1] Terminate. School zone, I'm not risking it.
- **L127** [Supervisor · P1] Weather's too bad for this. Terminate and shadow only.
- **L128** [Command · P1] We're out of cars. Terminate. Air, keep him if you can.

### 9.16 Weather, night, drawbridge, trains
- **L129** [Dispatch · P3] All units, rain's coming in heavy. Watch the bridges, the grate on the Iron Bridge is ice.
- **L130** [Dispatch · P3] Fog advisory, Bayview and Old Harbor. Slow it down.
- **L131** [Dispatch · P3] Night shift's on. Four cars for the whole south bank. Pick your fights.
- **L132** [Dispatch · P2] Kettle Street drawbridge lifting in one minute. Expect traffic stacked south.
- **L133** [Unit · P2] {unit}, bridge is up, he's stuck on the north side. Moving in.
- **L134** [Unit · P1] {unit}, he jumped the bridge. He actually jumped it.
- **L135** [Unit · P2] {unit}, train at Five Points, crossing's closed. I'm stuck.
- **L136** [Dispatch · P2] Freight moving through the yards, Westline crossing closed for two minutes.

### 9.17 Meets and illegal day races
- **L137** [Dispatch · P2] Reports of a street meet at {district}, a dozen cars. Units, Code 2, look don't touch.
- **L138** [Dispatch · P1] Caller says they're racing on {street} in broad daylight. All units.
- **L139** [Unit · P1] {unit}, it's a full race, {count} cars, {dir} on {street}. Daylight. Unbelievable.
- **L140** [Command · P1] Every unit we've got. Air up. Close the bridges.

### 9.18 Captain orders and acknowledgements
- **L141** [Command · P2] {unit}, hold {crossing}. Nobody crosses.
- **L142** [Unit · P2] {unit}, 10-4, holding {crossing}.
- **L143** [Command · P2] {unit}, set priority on the {colour} {class}. Ignore the others.
- **L144** [Command · P2] {unit}, get to Calloway north portal. {unit2}, south portal.
- **L145** [Unit · P2] {unit}, negative, I'm too far. Sending my position.
- **L146** [Command · P2] All units, stop chasing and start containing. Bridges and tunnel.

### 9.19 End of shift
- **L147** [Dispatch · P3] Shift change in ten minutes. Wrap it up.
- **L148** [Unit · P3] {unit}, heading back to {district} precinct, 10-7 at end of shift.
- **L149** [Dispatch · P3] Day units coming on. South bank's yours again.
- **L150** [Unit · P3] {unit}, long night. Coffee's on me.

---

## 10. Line-selection rules (06 emits, 13 plays)

1. 06 emits a `PoliceEvent::Radio` for every dispatch assignment and state change (06 prompt);
   the line id (e.g. `L066`) should travel with it once `PoliceEvent` is typed (v0.2.0), so audio
   and UI can look up voice/priority without parsing text.
2. Each situation has several lines; pick with the seeded sim RNG, never the same line twice within
   the last **8** lines of the same situation, and never the same line within **60 s** overall.
3. Day/night variants: L045, L048, L131 are night-only; L050, L149 day-only; L060 needs fog;
   L129 rain; L022 and L126 school hours only.
4. Racers hear lines through the scanner with a **3–10 s delay by scanner tier** (06 query,
   08 tool tier); the captain and cop players hear them live.
5. Queue: P1 interrupts P2/P3; at most 2 queued lines per tier; stale P2/P3 lines (> 10 s old) are
   dropped, P1 never.
6. Token fallbacks: missing `{cross}` → use `{district}`; missing `{plate}` → drop the clause.
7. No line may contain a brand, model or real place name; describe cars by colour + body style.
