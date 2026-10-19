# PROMPT 13 — Audio: engine, collisions from physics, sirens, scanner (`wp_audio`, `apps/testbed-web/src/audio/`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`. You own a new crate `wp_audio` (pure logic, no audio backend) and `apps/testbed-web/src/audio/` (Web Audio backend). Branch `ws/audio`. Do not edit other crates.

## Goal
Audio driven entirely by simulation state and `SimEvent`s so it is free content and always truthful: engine note from rpm/load, tire scrub from slip, suspension thumps, metal deformation and glass from contact impulses and lattice events, sirens with correct patterns, and a radio scanner voice feed from `PoliceEvent::Radio` lines.

## Work
- `wp_audio`: converts `VehicleState`, `FfbOutput`-style telemetry and drained events into an `AudioCommand` stream (play/stop/set-param on named voices, with priority and max-voices culling, distance attenuation and Doppler inputs computed here deterministically). Engine model: per-car `EngineAudioDef` (cylinders, firing order feel, induction/exhaust balance, turbo/supercharger flags) → granular-synthesis layer selection (idle/low/mid/high/limiter, on/off throttle) + crossfades; no real samples required yet — define the sample-pack contract (`data/audio/packs/<id>.json`) and ship procedural placeholders (oscillator+noise synth) so every car makes a sound today. Collision audio: map impulse magnitude × material kind (from lattice region) to layers (plastic crack, steel dent, structural crunch, glass shatter, scrape loops for sustained contact). Sirens: wail/yelp/phaser/hi-lo with real cadence; horn; tire squeal from combined slip above threshold scaled by load; surface rumble by `SurfaceType`.
- Scanner: queue radio lines by priority, 3–10 s delay by tier (from `wp_police` queries), chop/duck music, placeholder TTS via browser `speechSynthesis` behind a radio EQ; design for later recorded packs.
- Web Audio backend: voice pool, 3D panner per entity, listener from camera, mixer buses (engine, world, police, ui, music) with debug sliders.
- Tests: command stream determinism; voice culling respects limits; engine layer selection monotone in rpm.

## Deliver
Crate, backend, `data/audio/` contract + placeholders, README (sample-pack spec for a sound designer), `docs/integration/audio.md` (needs: per-region material on contact events — request a `ContactImpulse.material_kind` contract field if absent).
