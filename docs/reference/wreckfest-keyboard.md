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
2. 37.5 s: **Right + handbrake** (throttle off) for 1.0 s. The car pivots ~150–180° around the hairpin at 1st-gear speed (~5–10 mph). Rotation is free and fast.
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
