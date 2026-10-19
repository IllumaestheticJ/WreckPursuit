# 3-minute demo script: a night chase in the city

**Who this is for:** anyone showing or judging the single-player demo. No programming needed.
**What it is:** one player, one car, the procedural test city at night, normal traffic, AI police.
The player speeds, gets chased, escapes or gets caught, and then sees the repair bill.

The whole game in one loop: *drive → break the law → get chased → pay for it.* If those three
minutes feel right, the core of WreckPursuit works.

---

## Before you start

| Setting | Use | Why |
|---|---|---|
| City | Procedural test city (about 2 × 2 km grid, ring road, highway down the middle) | The only city with roads and traffic today. The real city (Port Vesper, `CITY.md`) comes later. |
| Time of day | 23:30 (night) | Fewer police, slower backup, quieter streets: the chase has room to breathe. |
| Car | Shiro Turbo (rear-wheel-drive sports coupe) | Quick enough to outrun a patrol car, light enough to dent visibly. |
| Damage mode | Arcade | Survives a hit or two, so you reach the garage with a bill instead of a wreck. |
| Assists | ABS and traction control on | Smooth for an audience; turn them off in playtests. |
| Camera | Chase camera | Shows the car and the dents. |

Where to play: the published testbed link in `docs/STATUS.md` ("What you can play right now"), or
`just web` on a dev machine.

---

## The script, moment by moment

Times are approximate. "Proves" is what the moment shows the viewer about the game.
"Ready?" says whether it works on `main` today (2026-10-19).

### 0:00 – 0:20 · A quiet city at night
**Show:** start parked on a side street. Street lights, a few cars moving, traffic lights cycling.
Pull out gently.
**Point out:** the city is alive even when nothing is happening. Cars stop at red lights, wait
their turn, use their indicators.
**Proves:** the world exists on its own; it isn't a race track with scenery.
**Ready?** Yes. City, traffic and signals run today.

### 0:20 – 0:50 · Drive like a normal person
**Show:** drive at the speed limit for half a minute. Brake for a red light, make a turn, follow
a car.
**Point out:** the car has weight. It leans in corners, dips under braking, and the tires squeal
if you push. Nothing happens to you because you're obeying the law.
**Proves:** handling is a real simulation, and being legal is safe.
**Ready?** Yes (handling). Engine and tire sound exist but are not hooked into the browser yet.

### 0:50 – 1:10 · Break the law in front of a witness
**Show:** turn onto a wide avenue, floor it to about twice the limit, run a red light right past
a police car.
**Point out:** the heat meter jumps. You are now wanted, at a low level. One mistake does not
bring the whole police force.
**Proves:** police react to what they actually see. Getting caught speeding is a choice you made,
not a random event.
**Ready?** Partly. The heat rules exist; showing them in the demo needs the police wired into the
game loop (the lead's next run).

### 1:10 – 2:10 · The chase
**Show:** the police car follows, keeping its distance rather than ramming. Weave through traffic.
At some point, clip something: a parked car, a pole, a civilian's bumper.
**Point out:**
- The police **shadow** you and wait for backup. At night backup takes one to two and a half
  minutes, so you have a window.
- Your car is now **dented where it hit**, and it may pull to one side. That dent stays.
- A police car that drives recklessly can wreck itself. Nobody is invincible.
**Proves:** chases are tense rather than arcade bumper cars, and damage is real and lasting for
both sides.
**Ready?** Damage: yes, it's the best thing to show today. Police chasing in the car: no, needs the
same wiring as above.

### 2:10 – 2:40 · Escape or capture (two possible endings)
**Ending A, escape:** get out of the police car's line of sight (turn behind a block of
buildings, take the highway) and stay out of sight. At night, at a low wanted level, about
10–15 seconds unseen is enough. The heat meter cools and the radio calls off the search.
**Ending B, capture:** stop, or get boxed in. If you're nearly stopped for 4 seconds with a
police car within 15 metres, you're caught.
**Proves:** the racer wins by breaking contact, not by being faster. The police win by patience.
Both endings are fair and readable.
**Ready?** No, needs the police wiring. Rules and numbers already exist in the police data files.

### 2:40 – 3:00 · The bill
**Show:** the garage screen. The car's damage is listed part by part with a price. Choose
**Repair all**, **Repair some**, or **Drive it damaged**. After a capture, the fine and impound
fee are added.
**Point out:** this is why you drive carefully. In Arcade the repair costs half what it does in
Simulation. Driving damaged is free, and the car keeps pulling left.
**Proves:** every crash has a cost you carry into the next session.
**Ready?** The pricing and the garage screen both exist; connecting the crash to the bill in the
demo is on the lead's list.

---

## If something goes wrong during a live demo

| Problem | What to do |
|---|---|
| No police car nearby | Run another red light near a busier junction, or restart from the start point. |
| You wreck the car in the first minute | That's a valid demo of damage. Jump straight to the bill. |
| The chase drags on | Stop and accept capture: it shows Ending B and the fine. |
| The browser stutters | Close other tabs; the demo is tuned for a normal laptop. Note it for the playtest log. |

---

## Playtest checklist

Use this when you (or friends) play the demo. You don't need to explain *why* something feels
wrong; describe what happened and when. A short screen recording or the replay file helps a lot.

Rate each line **1–5** (1 = broken, 3 = fine, 5 = great) and add a comment for anything at 3 or below.

### Handling feel (does the car feel like a car?)
- [ ] Does the car feel heavy, or does it feel like a toy?
- [ ] Does it turn when you expect, without snapping sideways for no reason?
- [ ] Can you tell when the tires are about to lose grip (sound, the car starting to slide)?
- [ ] Does braking hard feel strong and controllable?
- [ ] With assists off, is it hard but learnable, or just frustrating?
- [ ] Do different cars feel different (try a heavy truck and a light hatchback)?
- [ ] Keyboard vs gamepad: is either one unpleasant?

### Crash feel (does damage look and feel right?)
- [ ] Does a small bump do small damage and a big hit do big damage?
- [ ] Do dents appear where you hit, and stay there?
- [ ] After a crash, does the car drive differently (pulls, wobbles, loses power) in a way you can understand?
- [ ] Do bits fall off when they should, and not when they shouldn't?
- [ ] Did anything feel unfair: a gentle touch that wrecked the car, or a huge hit that did nothing?
- [ ] Is the repair bill about what you expected for the damage you saw?

### Police fairness (do the cops play fair?)
- [ ] When you got wanted, did you understand why?
- [ ] Did the police ever appear out of nowhere right next to you?
- [ ] Did they ever drive impossibly fast or never crash?
- [ ] Could you tell how to escape (break line of sight, hide)?
- [ ] When you escaped, did you feel you earned it?
- [ ] When you were caught, did you feel it was your fault?
- [ ] At night, did it feel like fewer police than you'd expect in the day?

### The city
- [ ] Did traffic behave like real drivers (stopping, turning, pulling over for sirens)?
- [ ] Did any civilian car do something silly (stuck, driving through others, crashing into each other)?
- [ ] Could you find your way around?

### General
- [ ] Did the game ever stutter, freeze or slow down? When?
- [ ] Was anything on screen confusing?
- [ ] What was the most fun moment? The most annoying?
- [ ] Would you play another round?

### How to report
For each problem write: **time of day in game, car, what you were doing, what happened, what you
expected.** Example: *"Night, Shiro Turbo, 80 km/h into a parked car, front barely dented and
the car drove perfectly. Expected a bent wheel."*
