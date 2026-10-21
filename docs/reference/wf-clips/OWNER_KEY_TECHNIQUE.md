# The owner's keyboard technique (from the 2026-10-21 reference clips)

Key states read from the on-screen keyboard at 30 fps (`*_track.json`, `*_keys.json`).
The 16:37 clip (Hammerhead RS, second run) has the steering wheel mostly out of view, so it is
keys only.

| clip | car | length | steer presses | median press | p25–p75 | longest | throttle held | throttle presses (median) | handbrake (Space) total |
|---|---|---|---|---|---|---|---|---|---|
| 16:30 | Rocket RX | 76 s | 74 | 0.23 s | 0.13–0.63 s | 3.6 s | 72 % | 32 (1.27 s) | 1.0 s |
| 16:34 | Hammerhead RS | 69 s | 55 | 0.23 s | 0.13–0.53 s | 5.7 s | 82 % | 13 (4.33 s) | 1.5 s |
| 16:37 | Hammerhead RS | 63 s | 50 | 0.40 s | 0.20–1.03 s | 8.2 s | 74 % | 40 (0.88 s) | 1.9 s |
| 16:39 | Hammerhead RS, **TCS Half** (owner: "far more controllable") | 67 s | 78 | 0.38 s | 0.20–0.77 s | 3.3 s | 81 % | 34 (0.98 s) | 1.6 s |

So: mostly short taps (≈0.1–0.6 s) with occasional long holds (3–8 s through long drifts), the
throttle held most of the time, and the handbrake rarely. Benches should drive like this — the
`*_track.json` key timelines can be replayed directly as keyboard input.

## Hands-off countersteer is capped (owner observation, confirmed)
Settled hands-off wheel-marker angles (> 0.5 s after the last steer key, throttle on):
Rocket RX p50 16°, p90 24°, max 34°; Hammerhead RS p50 28°, p90 45°, max 47°. Held keys take the
wheel far past 90° (out of view). So the no-key self-alignment is limited to roughly a quarter to a
third of full lock even in big slides.
