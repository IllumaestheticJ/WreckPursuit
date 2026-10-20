# Impact & horn renders (owner row 74)

Offline renders of the director's command stream through the testbed's real voice pool and
mixer (`apps/testbed-web/e2e/audio_impacts.mjs`), stereo 24 kHz / 16 bit, default settings
(recorded samples + impact / horn synthesis). Crashes are real sim runs (simcli scenarios,
Rapier contacts, `wp_damage` lattice) and the full mix (engine, tyres, rolling noise included);
the glass and horn scripts mute the idling car's engine bus so the subject is clear.

| File | What |
|---|---|
| `frontal_20kmh.wav` | debug_sedan into a rigid barrier at 20 km/h (Arcade), ear 8 m to the side |
| `frontal_50kmh.wav` | same at 50 km/h (a second, softer hit on the dented hood ~1.1 s later) |
| `frontal_90kmh.wav` | same at 90 km/h: bass transient, sidechain duck, dense crunch, windshield breaks |
| `frontal_50kmh_interior.wav` | 50 km/h from the driver's seat: more body, less air, other sounds muffled |
| `sideswipe_scrape.wav` | 80 km/h sedan leans into a 43 km/h one (car-to-car scrape), roadside ear |
| `wall_scrape.wav` | 70 km/h sedan glances into a wall and grinds along it ~3 s (stick-slip + sparks) |
| `glass_break.wav` | rear window (tempered) breaks at 0.2 s, windshield (laminated) at 1.6 s |
| `horn_disc_pair.wav` | car horn (two disc horns, 420 + 500 Hz) held 2.0 s |
| `horn_disc_pair_damaged.wav` | the same horn with a smashed front end: weaker, rattly, detuned |
| `horn_air_truck.wav` | truck air horn (three trumpets) held 2.0 s |
| `horn_air_police.wav` | police air horn (two trumpets) held 2.0 s |

Regenerate: `node apps/testbed-web/e2e/audio_impacts.mjs` (builds the trace with
`cargo run --release -p wp_audio --example impact_trace`; needs the wasm build only for its
live part, `--no-live` skips it).
