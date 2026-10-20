# Wreckfest keyboard handling — owner reference (target feel)

The owner wants WreckPursuit's keyboard driving to feel like Wreckfest's (rows 57, 90, 91, 98, 104–108).
This file collects the owner's reference material. Owner videos and notes will be added here as they arrive.

## Owner's Wreckfest assist settings (screenshot, 2026-10-20)

| setting | value |
|---|---|
| Driving difficulty | Custom |
| Shifting | Manual |
| ABS | Full |
| Traction control | **Off** |
| Stability control | **Off** |
| AI difficulty | Novice |

What this tells us: the owner plays Wreckfest on keyboard with traction and stability control **off**. So the controllability they want doesn't come from throttle or yaw-control systems. It comes from the keyboard steering itself:
- how fast the rack moves
- self-aligning / countersteer behaviour
- key ramping

This supports the decisions in rows 104/105 (TCS off = raw throttle; no hidden throttle cuts) and the steering-first approach in Drift Labs B/C. Manual shifting is the owner's normal setting too.

## Still to collect (owner is recording)

- The controls screen: keyboard steering sensitivity / speed / linearity / deadzone, if Wreckfest shows them.
- Clips at 60 fps with a key overlay, one rear-drive and one front-drive car:
  1. tap vs hold at ~100 km/h
  2. release in a slide
  3. hold countersteer in a slide
  4. handbrake entry + catch
  5. lift-off transition (S-bend)
  6. full-throttle launch / 2nd-gear exit
  7. lane change at ~100 km/h
- Matching Drift Lab C recordings (replay → csv) of the same moves.

## Clip 1 analysis (2026-10-20, owner lap)

The lap: a 57 s arena lap ("Madman Stadium", Last Man Standing) in a heavy rear-drive muscle car, keyboard on the **arrow keys**, Space = handbrake. 60 fps, 1080p, NohBoard overlay. The key states were read per frame from the overlay (3,425 frames).

**Inputs**

| | |
|---|---|
| Throttle (↑) | held **75 %** of the lap; 34 presses, median 0.88 s, up to 6.9 s |
| Brake (↓) | **never used** |
| Throttle while steering | on during **80 %** of steering time |
| Left / Right | ~31 % of the lap each; median press 0.37 s (L) / 0.60 s (R); p90 ~2.1–2.3 s; longest **4.3 s** |
| Both at once | never |
| Reversals (release → opposite key) | 0.03–0.95 s; about a third under 0.2 s, about half under 0.3 s |
| Handbrake (Space) | 8 taps, median 0.4 s (0.18–0.75 s) |

**What the car does.** In the 40.5–44.8 s stretch the player holds **Left** (opposite lock) with the throttle on for 4.3 s, while the car holds a **right-hand drift**. In the chase cam the nose points right of the travel direction, the tail is out to the left, and the body slip angle is roughly 30–45°, steady for several seconds. This is in 2nd gear at 27–47 mph (43–75 km/h). It doesn't spin.

**Targets for WreckPursuit keyboard (Drift Lab C)**

1. **Countersteer key held + full throttle = a sustained, steady drift** at ~30–45° in a powerful RWD car in 2nd gear. This is the core technique: the player floors it and holds opposite lock. In our driver tables, "Naive" (counter key + full throttle) must **hold the drift without spinning** at the default strength. Only truly excessive input (much more angle or a wrong key) should spin.
2. The envelope must cover ~45–50° comfortably at the default strength.
3. **Transitions are fast:** the opposite key often comes 0.03–0.2 s after release. The car must swing through and pick up the new side immediately (row 107).
4. Short taps (0.1–0.4 s) are used to adjust the angle. They need to have a visible but small effect.
5. Handbrake taps of ~0.4 s start drifts; the catch must be quick (row 108).
6. No brake in this kind of driving, so lift and throttle carry the speed control.

## Clip 2 analysis (2026-10-20, owner race lap)

The lap: a 92 s street-circuit race (lap 1/3) in a lighter rear-drive sports car (#44), arrow keys, Space = handbrake. Gears 1–6, up to ~110 mph.

**Inputs**

| | |
|---|---|
| Throttle | held **73 %**; median press 1.0 s, up to 9 s |
| Brake | 8 % (8 presses, median 0.6 s): used for big stops before tight corners |
| Left / Right | 26 % / 23 %; **taps dominate**: median 0.18 s (L) / 0.23 s (R), p10 0.08–0.10 s, p90 1.0–1.4 s, longest 2.4 s |
| Reversals | 60; median 0.30 s; **38 % under 0.2 s** |
| Throttle while steering | 67 % |
| Handbrake | 9 uses, median 0.37 s, longest 1.2 s |

**Handbrake hairpin (37.2–40.5 s), step by step**

1. 37.17 s: a **feint**: a 0.3 s Left tap on the throttle.
2. 37.5 s: **Right + handbrake** (throttle off) for 1.0 s. The car pivots ~150–180° around the hairpin; the speedo reads **~28 mph at handbrake-on**, falling 18 → 10 → 5 mph during the pivot (correction 2026-10-20: the entry is ~45–55 km/h, not 5–10 mph). Rotation is free and fast.
3. 38.5 s: **throttle back on**, still holding Right, while the handbrake is released.
4. 39.3 s: **Left (opposite lock) + throttle** for ~1 s. This catches the rotation and the car drives out of the hairpin in a short power slide.

**Targets for WreckPursuit keyboard (in addition to clip 1)**

1. **Racing steering is mostly taps of 0.1–0.25 s.** A tap must produce a modest, proportional steer, not a full-lock flick, and not nothing. The keyboard ramp, rack rate and speed-sensitive lock decide this. Compare our lane-change / tap response against "tap 0.2 s at 100 km/h".
2. **Handbrake + key into the turn = free, fast pivot** (~150–180° in about 1 s at low speed). Throttle back on plus opposite lock **catches immediately** (row 108: no 0.5 s grace).
3. **The feint (a short opposite tap before the turn-in)** should help load the car into the rotation, as it does physically. No assist should damp it out.
4. Normal racing uses the brake; drifting arenas don't. Both have to feel good.

## Clip 3 analysis (2026-10-20, owner race lap, front-wheel drive)

The lap: a 73 s street-circuit race (lap 1/3) in a front-drive touring car (#96), arrow keys, Space = handbrake. Up to ~105 mph in 5th.

**Inputs.** A much calmer style than the RWD laps.

| | |
|---|---|
| Throttle | held **78 %**: long, flat-out stretches (median press 4.6 s, up to 14 s) |
| Brake | 4 %: short, hard stops before slow corners (median 0.9 s) |
| Left / Right | only 8 % / 15 % of the lap; **almost all taps**: median 0.14–0.15 s, p10 0.08 s, longest 1.0–1.6 s |
| Reversals | 22; median 0.44 s (only 14 % under 0.2 s) |
| Throttle while steering | 79 % |
| Handbrake | 4 taps, median 0.35 s |

**Brake + handbrake corner (41.0–45.0 s), step by step**

1. 41.2 s: brake 1.3 s from ~101 mph to ~65 mph (5th → 3rd), with a brief Right tap while braking.
2. 42.5 s: Right, then **Right + handbrake 0.3 s** (off the throttle). The front-drive car's tail steps out ~30–40°.
3. 43.2 s: **full throttle** and a **0.35 s Left tap** (opposite lock). The driven front wheels pull the car straight. By ~44.5 s it is tidy again, exiting in 2nd at ~35–45 mph.

**Targets for front-drive cars**

1. FWD cars drive mostly on **short taps** at high speed: stability is the norm. A 0.15 s tap at 100+ mph gives a small lane-change-sized correction.
2. A handbrake tap (0.3 s) on turn-in rotates a FWD car ~30–40°. **Full throttle then pulls it straight** with a short countersteer tap: the front wheels drag the nose along the travel. That makes the FWD power-on "straightening" the main recovery, rather than a long countersteer hold.
3. Off throttle and turning in, a FWD car should rotate a little (lift-off), but not snap.

## Owner's keyboard "Advanced" controller settings (screenshot, 2026-10-20)

| setting | value |
|---|---|
| Controller | Keyboard |
| Vibration strength | 0 % |
| Steering dead zone | 0 % |
| Steering saturation | 100 % |
| Steering sensitivity | 100 % |
| **Steering speed sensitivity** | **75 %** |
| Accelerator / brake / clutch saturation | 100 % |
| Accelerator / brake / clutch dead zone | 0 % |

The screen also has **Test steering / throttle / braking / clutch** bars that show the processed input live. Recording them while tapping and holding keys gives Wreckfest's exact keyboard ramp rates (requested from the owner). "Steering speed sensitivity" is the same idea as our `steer_speed_sensitivity` (less lock at speed): the owner's value is 75 %.

## Clip 4 analysis (2026-10-20, heavy RWD muscle car, **TCS Half**)

The owner's difficulty for this lap: Custom, Manual, ABS Full, **Traction control Half**, Stability Off. A 70 s street race in a heavy rear-drive muscle car (#49), with a stretch of cockpit camera.

**Inputs**

| | |
|---|---|
| Throttle | 80 % (median press 0.65 s) |
| Brake | 2 % |
| Left / Right | 24 % / 20 %, median 0.19 / 0.31 s taps, longest 2.9 s |
| Handbrake | 4 taps, ~0.3 s |

- **Launch:** the player feathers the throttle key at the start (0.8–2.8 s: six taps of 0.1–0.4 s), then holds. With TCS Half there is still **visible wheelspin and smoke** in 1st and into 2nd (28–45 mph), and a little tail movement. That matches our planned **TCS Low** (row 105). Wreckfest's levels are **Off / Half / Full**, so we should use the same names.
- **Racing:** like clip 2, short taps and frequent quick corrections. Throttle stays on through most corners.

## Run 5 setup (screenshots, 2026-10-20)

**Difficulty:** Custom, Manual shifting, **ABS Half**, TCS Off, Stability Off. So Wreckfest's ABS also has Off / Half / Full levels.

**Car tune** (the same #49 muscle car, on 5-step sliders):

| tune | setting |
|---|---|
| Suspension | **Stiff** (5/5) |
| Gear ratio | **Long** (5/5) |
| Differential | 4/5 (between Limited and **Locked**) |
| Brake balance | 2/5 (toward **Rear**) |

The car's stat bars show Acceleration 10.0, Top speed 10.0, Cornering 6.7.

**Gaps this shows for WreckPursuit**
- ABS levels Off / Half / Full (we have on/off).
- A **per-car tune screen** with 5-step suspension, gear ratio, differential and brake balance. This is separate from the mod tree, is free, and is set per event/garage.
- Car stat bars (acceleration / top speed / cornering).

## Clip 5 analysis (2026-10-20, #49 muscle car, tuned, **ABS Half**, TCS Off)

The lap: 69 s, same street circuit and car as clip 4, but with the tune from "Run 5 setup" (stiff, long gears, diff 4/5, rear brake bias) and ABS Half.

**Inputs**

| | |
|---|---|
| Throttle | 71 % |
| Brake | 3 % (median 0.5 s) |
| Left / Right | 24 % / 20 %, median 0.18 / 0.28 s taps |
| Handbrake | 7 uses, median 0.23 s, often **with the throttle held** |

**Brake → handbrake-stab → power-drift corner (41.0–45.0 s), step by step**

1. 41.0–42.1 s: two brake presses (0.2 s + 0.5 s) with a Left tap in between, 82 → 57 mph (3rd → 2nd). With ABS Half and rear brake bias the car stays straight and settles.
2. 42.2 s: **throttle + Left**, then **throttle + Left + handbrake stabs** of 0.2–0.3 s and 0.1 s while staying on the throttle. Each stab adds rotation; the rear doesn't lock the car dead.
3. 44.0 s: **Right (opposite lock) + throttle**: a held ~20–30° power drift at 52–55 mph in 2nd through the corner exit.

**New targets**
1. **Handbrake stabs on the throttle** (0.1–0.3 s, throttle and steer key held) must add rotation without killing the drive. Check our row-56 handbrake declutch (auto clutch opens with the handbrake) against this. For short stabs with the throttle held, the reference keeps power, or recovers it instantly.
2. **ABS Half + rear brake bias:** braking at 80+ mph with a steering tap stays controllable and settles the car. A tap while braking gives a mild rotation, not a spin.
3. With the tune (stiff, locked diff, long gears) a 2nd-gear power drift at ~50–55 mph holds steadily with opposite lock and throttle.

## Keyboard input is raw in the reference (owner test, 2026-10-20)

The reference game's "Test steering" bar on the keyboard jumps straight between **−100 % and +100 %**, with no ramp or smoothing at the input layer. All the keyboard smoothing must therefore happen **inside the car's steering model** (the rack's slew rate, speed-sensitive lock and self-alignment), not in the input filter.

What this means for WreckPursuit:
- Keyboard steering input should reach `DriverInput.steer` as a plain −1 / 0 / +1. Our testbed key ramp (`apps/testbed-web/src/input.ts`: 4/s in, 12/s out; drift-twitch's 6/s in and out) should go, or become ~instant. Rate limiting belongs in `wp_vehicle` steering, where it can behave differently for turn-in, return to centre, countersteer and slides.
- Measuring the reference's actual front-wheel rate needs a camera that shows the front wheels, or the full cockpit steering wheel (owner asked).

## Reference car specs (owner screenshots)

The reference game's car names are kept out of this repo (rule 9); cars are labelled by role.

| ref | role | power | torque | weight | gears | drive | class |
|---|---|---|---|---|---|---|---|
| **REF-RWD-A** (#49, clips 4–5) | full-size RWD muscle | 462 hp (345 kW) @ 4800 rpm | 585 lb-ft (793 N·m) @ 3200 rpm | 3018 lb (1369 kg) | 5 | RWD | A, rating 337 |
| **REF-RWD-B** (#44, clip 2) | midsize RWD sports | 340 hp (254 kW) @ 6400 rpm | 299 lb-ft (405 N·m) @ 4200 rpm | 2308 lb (1047 kg) | 6 | RWD | A, rating 324 |
| **REF-RWD-D** (clip 1 arena car, same livery as the clip 1 frames) | full-size RWD muscle | 523 hp (390 kW) @ 5800 rpm | 595 lb-ft (807 N·m) @ 3800 rpm | 3600 lb (1633 kg) | 4 | RWD | A, rating 320 |
| **REF-FWD-C** (#96, clip 3) | compact FWD touring | 162 hp (121 kW) @ 7900 rpm | 132 lb-ft (179 N·m) @ 5800 rpm | 2164 lb (982 kg) | 5 | FWD | C, rating 164 |

The reference RWD cars sit at about **239–252 W/kg** (REF-RWD-D: 239), and both are light for their class. Our nearest analogues are heavier and less torquey:

| ours | mass | power | torque | W/kg |
|---|---|---|---|---|
| rampart_50 (vs REF-RWD-A) | 1753 kg | 319 kW | 529 N·m | ~182 |
| lucciola_350 (vs REF-RWD-B) | 1345 kg | 278 kW | 363 N·m | ~207 |
| kestrel_ts (vs REF-FWD-C, ~123 W/kg) | 1453 kg | 181 kW | 335 N·m | ~125 (similar ratio, but ~470 kg heavier) |

That is a likely part of the feel gap: the reference cars have more torque per kg, so they light up the rears and power-oversteer far more readily. The parity suite should measure the reference specs directly. One way is a parity-only test car JSON with the reference numbers, kept out of the player roster or given analogue names, so physics behaviour can be compared like for like.

## Steering-wheel timing (clip 7, cockpit view, standing still, 2026-10-20)

The owner recorded the cockpit steering wheel at a standstill with the key overlay. A spoke on the wheel shows its rotation frame by frame (60 fps).

| test | key | wheel response |
|---|---|---|
| hold Left 1.23 s (5.27–6.50 s) | raw, on | the spoke starts moving ~1 frame after the key; it sweeps at a near-constant fast rate and reaches the final lock ≈ **0.25–0.30 s** after the press, with a small settle; it holds steady after that |
| release (6.50 s) | raw, off | returns to centre over ≈ **0.30–0.35 s**, again near-constant rate |
| tap Left 0.23 s (3.50–3.73 s) | raw | reaches **near-full lock** by the time of release, then returns in ~0.3 s |

**Conclusion.** The reference keyboard steering at standstill is a **simple constant-rate slew**: full lock in ~0.28 s, back in ~0.33 s. There is no input smoothing (row "keyboard input is raw"), and no visible easing or intent logic. So a 0.2 s tap is most of a full-lock flick at low speed. At speed, "steering speed sensitivity 75 %" scales the lock down (not yet measured at speed).

This contrasts with our Drift Labs, which layer intent modes (hold / angle-up / transition / catch). Those may be the reason the owner says Drift Lab C "is nothing like" the reference: the reference likely gets its feel from **the car (tyres, weight, torque) plus a plain steering rate**, not from a smart assist.

## Clip 8 (cockpit race lap, #49 car) — steering at speed

The key overlay sits at a new position in this clip (arrows at x≈444–508, y≈1034–1066). Holding Right at ~96 mph (3rd gear, braking from 98 to 84 mph, 19.25 s onward): the wheel spoke reaches its final position **≈0.23 s** after the key press. That is the same constant-rate slew as at a standstill. The visible steering-wheel angle at 96 mph looks similar to the standstill lock, so "steering speed sensitivity" may act on the road wheels / tyre forces rather than on the cockpit wheel animation. **Unconfirmed**: a view of the front road wheels is still needed to measure the lock at speed.

## Owner feedback: our suspension "feels far too switchy" (2026-10-20)

In the reference footage the cars' body roll and pitch build and settle **smoothly and relatively slowly**, with heavy-feeling weight transfer and no quick back-and-forth rocking on direction changes. Our cars swap weight too quickly and too nervously. Targets: lower roll/pitch natural frequency, or more damping on transients, measured as roll angle vs time in a 60 mph lane change and an S-bend. Owners: Drift Lab W (lab feel / A-B) and machine A (ride model).

## Clip 5 note: ABS levels

The owner's clip 5 uses **ABS Half**. WreckPursuit should offer ABS **Off / Half / Full**, alongside TCS Off / Half / Full (assigned to machine A).

## Clip 8: the steering wheel during a drift — the reference steers relative to the slide (2026-10-20)

The owner's hypothesis is that the reference has **no throttle limiting at all, and all stabilisation is in the steering**. The cockpit clip supports the steering half.

Clip 8, 34.15–37.0 s: a left hairpin in 2nd at 30–37 mph, power-sliding (the view swings left with motion blur; tail out to the right). The **Left key is held continuously for 2.9 s** (with throttle, plus 2 handbrake stabs). The cockpit wheel:

| t after Left pressed | Left key | wheel |
|---|---|---|
| 0.1 s | held | full left lock (spoke at the top, as at standstill) |
| 0.5–1.1 s | held | **back near centre** while the car rotates into the slide |
| 1.5–2.0 s | held | about half left |
| 2.4–2.8 s | held | **past centre toward the right**, i.e. countersteer |
| released | — | centre |

So in a slide the reference's front wheels do **not** follow the key: they are pulled toward the direction of travel, and the key only adds an offset. At a standstill (clip 7) the same key gives a plain full lock. This is consistent with **velocity-relative steering**: front-wheel angle (body frame) ≈ −(front-axle slip direction) + key × lock(speed), clamped to the steering lock. Meaning:
- the key steers relative to where the car is *going*
- releasing the keys points the wheels along the travel (a natural catch / hold)
- the counter key goes further into opposite lock

That explains why counter key + full throttle holds a steady drift in clip 1, and why holding the into-turn key mid-slide doesn't spin the car.

Throttle: no visible limiting. Clip 5 floors it from rest (TCS off) and goes straight, and keyboard Test Throttle is raw 0/100 %.

## Weight transfer speed (owner: "the car weight transitions way too fast", 2026-10-20)

Reference, clip 4 (#49 muscle car, chase cam, 8.1–9.6 s): a Right key press at 8.3 s at ~50 mph with a brake dab. Read from the rear bumper and tail-light line in 10 fps frames:
- **No visible body roll for ~0.2–0.3 s** after the key.
- Roll builds **smoothly over ~0.8–0.9 s** to its peak, about 1.1 s after the press.
- **Large peak roll, roughly 5–7°**, and the car leans visibly onto its outside wheels.
- **No overshoot or rocking.** The body holds the lean and the tail slides out on top of it.
- Heading change starts ~0.3–0.4 s after the key and builds over about a second.

Targets for WreckPursuit (lab first, then car data):
- roll natural frequency ~0.9–1.1 Hz (ours: 1.5 Hz lab default, 1.76 Hz data)
- near-critical damping (ζ ≈ 0.9–1.1)
- 10→90 % roll rise time ~0.6–0.8 s in a 100 km/h lane change
- peak roll ~5–7° at about 0.8 g
- the same slower build for pitch under braking and throttle

This needs softer roll stiffness (springs + bars) **with** enough travel or ride height to stay off the bump stops, more roll inertia if the body inertia is low, and a slower yaw build.

## Letting go of the steering mid-drift (owner, 2026-10-20)

Owner: **in the reference, releasing the steering keys mid-drift makes the car slowly spin.** It does **not** straighten or hold the drift. Clips agree:
- **Clip 1, 44.7 s:** after a 4.3 s Left hold (a right-hand drift), the keys are released for 0.8 s with the throttle on. The car keeps rotating, and the tail keeps coming round until Right is pressed at 45.5 s.
- **Clip 1, 21.5 s:** keys released after a 1.1 s Left hold. The car keeps yawing round at falling speed.

Combined with the clip-8 cockpit finding (the wheel is pulled toward countersteer *while a key is held*), this gives the target behaviour for keyboard:
- **Key held** (either direction) during a slide: the wheels are pulled toward the travel direction (velocity-relative), plus the key's offset. Holding the key keeps the drift controllable.
- **No key:** the wheels return to the body's centre at the normal return rate, with **no** automatic countersteer. A powered RWD slide then keeps rotating **slowly** (a gradual over-rotation into a spin over ~1–2 s, not a snap).
- So the player must keep a key held to keep the drift. Letting go is a mistake that spins you slowly.

## Grip-driving balance: ours understeers far too much (owner, 2026-10-20)

Owner: our cars "still understeer a ton when not drifting". What the reference clips show in normal grip cornering:
- **Short taps (0.1–0.25 s) turn the car in promptly.** The nose follows the key at once; there's no "push wide, then bite".
- **Lifting at turn-in rotates the car** (clip 3 FWD and clips 2/4 RWD), and power in a corner brings the tail round on RWD.
- **Clip 8 (cockpit, ~96 mph):** a held key takes the wheel to about the same visible angle as at a standstill, and the car turns hard at that speed. Speed sensitivity doesn't appear to cut the road-wheel lock much. Our default "rack" mode cuts lock to 63 % at 100 km/h and 43 % at 150, which on its own produces strong understeer at speed.

Targets:
- near-neutral steady-state balance (understeer gradient ≈ 0–1 deg/g in a 60 m circle at 60 and 100 km/h, measured with the parity suite)
- a lift-off yaw kick at turn-in
- RWD power-on oversteer
- tap response matching the reference (heading change starting within ~0.15 s)

Suspects, in order:
1. the rack-side speed sensitivity (try "tyre" mode or a much gentler curve)
2. front vs rear tyre peak grip / cornering stiffness and load sensitivity
3. the roll-stiffness split, since lab bars ×0.25 and data ARB front/rear bias both matter
4. max lock
5. static weight distribution and CoG height in the car data
