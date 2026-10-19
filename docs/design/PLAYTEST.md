# Playtest feedback form: greybox demo

Fill this in after playing the published testbed (link in `docs/STATUS.md`, "What you can play
right now"). It takes about 10 minutes. The script you are testing is `DEMO_GREYBOX.md`.

**How to use it:** copy this file to `docs/design/playtests/YYYY-MM-DD-<initials>.md`, or paste it
into an issue, and fill in the blanks. You don't need to explain *why* something feels wrong:
say what happened, when, and what you expected. A screen recording or the replay file
(from the testbed's replay panel) helps a lot.

**Scores:** 1 = broken, 2 = bad, 3 = fine, 4 = good, 5 = great. Add a comment for anything at 3 or below.

---

## 1. The session

| | |
|---|---|
| Date | |
| Tester | |
| Build (the date you opened the link, or the commit if you know it) | |
| Browser + computer | |
| Input | keyboard / gamepad (which one?) |
| Car | |
| Damage mode | Arcade / Simulation |
| Assists | on / off |
| Runs played | |
| How each run ended | escaped / captured / wrecked / gave up / something broke |

## 2. Did each beat happen?

Tick what you saw. "Not yet" is a fine answer: some beats are known to be missing
(`DEMO_GREYBOX.md` §5).

| # | Beat | Happened? | Comment |
|---|---|---|---|
| 1 | **city chase** loaded and I was driving within a few seconds | yes / no | |
| 2 | Traffic and traffic lights were moving | yes / no | |
| 3 | My heat went up when I sped past a police car | yes / no | |
| 4 | A police car started chasing me | yes / no | |
| 5 | The screen told me I was being chased | yes / no | |
| 6 | Sirens came on when the chase started | yes / no | |
| 7 | I broke line of sight and the screen showed I was being searched for | yes / no | |
| 8 | I escaped, and the screen said so | yes / no / didn't try | |
| 9 | I got caught, and the screen said so | yes / no / didn't try | |
| 10 | A repair bill appeared (on its own, or with **F**) | on its own / with F / no | |
| 11 | The bill showed a fine and an impound fee after a capture | yes / no / not captured | |
| 12 | **Repair all** fixed the car | yes / no / didn't try | |

## 3. The four core questions

These decide whether the greybox is done. Score each 1–5.

| Question | Score | Comment |
|---|---|---|
| **Does the car feel like a car?** (weight, grip, braking, sliding) | | |
| **Do crashes look and feel right?** (dents where you hit, the car drives worse after) | | |
| **Are the police fair?** (you knew why you were wanted, they didn't cheat, escape was possible) | | |
| **Did the bill feel deserved?** (about what you expected for the damage and the bust) | | |

## 4. Details (score only the ones you noticed)

### Driving
| | Score | Comment |
|---|---|---|
| Steering responds when I expect, without snapping sideways | | |
| I can tell when the tires are about to lose grip | | |
| Braking is strong and controllable | | |
| Keyboard / gamepad feel OK | | |

### Damage
| | Score | Comment |
|---|---|---|
| Small bump = small damage, big hit = big damage | | |
| Parts fall off when they should, and only then | | |
| After a crash I understood why the car drove differently | | |
| Nothing felt unfair (a touch that wrecked me, or a huge hit that did nothing) | | |

### Police
| | Score | Comment |
|---|---|---|
| I understood what raised my heat | | |
| Police never appeared out of nowhere next to me | | |
| Police never drove impossibly fast or rammed parked / stopped cars | | |
| I could tell how to escape | | |
| Being caught felt like my fault | | |
| At night it felt quieter than I'd expect by day | | |

### City and traffic
| | Score | Comment |
|---|---|---|
| Traffic behaved like real drivers (stopping, turning, pulling over for sirens) | | |
| No civilian car got stuck, drove through others or crashed on its own | | |
| I could find my way around | | |

### Screen and performance
| | Score | Comment |
|---|---|---|
| The heat / chase readout was clear | | |
| The bill was easy to read | | |
| No stutters, freezes or slowdowns | | |

## 5. Problems

One row per problem. Example row filled in.

| Game time | Car | What I was doing | What happened | What I expected | Recording / replay? |
|---|---|---|---|---|---|
| *23:14* | *Shiro Turbo* | *stopped at a red light, wanted L1* | *a police car drove into my back at 40 km/h* | *it stops behind me and waits* | *replay saved* |
| | | | | | |
| | | | | | |
| | | | | | |

## 6. Overall

- Most fun moment:
- Most annoying moment:
- One thing to fix first:
- Would you play another round? yes / maybe / no
- Anything else:

---

## For the team: where answers go

| Section / line | Owner |
|---|---|
| Driving | A (vehicle), H (car tuning) |
| Damage | B (damage), H (lattices) |
| Police, beats 3–9 | E (police), D (shared controller), J (sirens) |
| City and traffic | D (world) |
| Heat readout, banners, bill screen, beats 5, 7–11 | C (testbed), M (UI kit) |
| Bill amounts, fine, impound, beat 11–12 | G (economy), B (`apply_repair`), lead (wiring) |
| Performance | I (harness), C |
| Anything else | lead triages; design questions to P (`docs/design/`) |

The lead copies each problem row into the owning machine's integration doc under "Lead feedback".
