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
