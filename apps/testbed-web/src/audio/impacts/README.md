# Impact, crunch, scrape and horn synthesis (owner row 74)

Browser side of "more accurate, harder-hitting impact sounds and better horns". The Rust
director (`crates/wp_audio/src/impact.rs`) turns the physics into parameters; these files turn
the parameters into Web Audio node graphs. Native nodes only (oscillators, biquads, wave
shapers, buffer sources): no AudioWorklet is needed at these voice counts, and the graphs run
the same in `OfflineAudioContext` for the renders.

| File | Synth / what |
|---|---|
| `index.ts` | `ImpactSynths`: the voice pool's hook (`VoicePool.play` asks it first), the impacts setting, the impact voice log |
| `body.ts` | `impact_body`: modal panel bank + contact excitation + chassis thump + bass transient + sidechain; `kind = 1`: glass break |
| `crunch.ts` | `crunch`: granular loop over the recorded metal crunches (noise grains in procedural mode) |
| `scrape.ts` | `friction`: stick-slip scrape layered on the recorded scrape loop, sparks |
| `util.ts` | seeded RNG, noise buffers, envelopes, asymmetric soft clip |
| `../horn.ts` | `disc_horn` (car: two electromagnetic disc horns) and `air_horn` (trucks, police) |

## Setting

Audio panel → **impacts & horns: synth / recorded** (persisted, `AudioSystem.setImpacts`).
`synth` (default) layers the synthesis on the recordings and plays recorded horn cues through
their procedural twin's horn model; `recorded` is the previous behaviour (`impact/body` and
`impact/crunch` skipped, recorded horns and scrape loops as they were). Procedural cues that
name these synths always use them.

## `impact_body` parameters (Play-time `synth` map from `impact::body_params`)

| key | meaning |
|---|---|
| `kind` | 0 body hit, 1 glass break |
| `region` | `PanelClass` code: 0 bumper, 1 hood, 2 wing (fender / quarter), 3 door, 4 roof, 5 trunk, 6 glass, 7 structure |
| `strength` | excitation 0..1: log of `½·m_eff·v²` between `energy_floor_j` and `energy_full_j`, or the tick's impulse |
| `bright` | 0..1, hardness of the contact (closing speed; plastic dulls it) |
| `contact_ms` | excitation burst length (short for fast, hard contacts) |
| `f0`, `modes` | fundamental of the struck plate (Hz, × car size) and number of modes (4–6) |
| `ring_s` | decay time constant of the fundamental (higher modes decay faster: `∝ (f0/f)^0.6`) |
| `inharm` | random mode detune (a dented panel is no longer one coherent plate) |
| `buzz` | 0..1 damage buzz: asymmetric clipping + a 26–48 Hz chop of the ring |
| `plastic` | 0..1: dull, damped, with a crack band |
| `thump`, `thump_hz` | chassis thump level and pitch (lower for heavier cars) |
| `transient` | bass-boosted crack for heavy hits (+12 dB low shelf, 48–110 Hz drop) |
| `duck`, `duck_s` | sidechain depth on the Engine / Police / Music buses (× how loud the hit is at the listener) and hold time |
| `interior` | listener in this car's cabin: the air-borne part is low-passed, the thump is stronger |
| `seed` | cosmetic scatter |
| glass: `tinkles`, `tail_s` | number of debris fragments and tail length (tempered rear / side glass > laminated windshield) |

Mode ratios (`PLATE`) are those of a simply supported plate of aspect ≈ 1.3, a little compressed
for curvature stiffening. Each mode is a sine with a 1.5 ms attack and an exponential decay
(the impulse response of a resonator bank), starting 2.5 % sharp and settling in 40 ms (the
panel stiffens for a moment under the hit). Per voice: ≤ 6 mode oscillators, 2 thump
oscillators, a transient oscillator and two noise bursts; everything is scheduled at creation
and stops itself (≤ 2.5 s).

## `crunch` (looped, slot `CRUNCH` = 0x1A2)

`load` = log of the lattice's plastic power (0..1), every frame. Grains of 18–90 ms are
scheduled up to 50 ms ahead of the audio clock at 12–170 grains/s (∝ load²), each a random slice
of the recorded `impact/structural_crunch` / `impact/panel_dent` files at 0.8–1.25 × `pitch`
(heavier cars lower), ≤ 24 overlapping. A brightness low-pass follows the load. The voice is
`everyFrame` so the pool calls `update` even when nothing changed.

## `friction` (scrape loops)

Per frame: `freq_hz` = stick-slip rate (Hz, Doppler included) from the sliding speed, `load` =
scrape level, `pitch` = rate of the recorded loop. Play-time: `metal`, `sparks` (metal on the
static world), `force` (contact pressure). A jittered sawtooth at the stick-slip rate, clipped
asymmetrically, drives 3–4 panel resonances (metal: 0.8 / 1.7 / 3.1 / 4.6 kHz, Q 9; plastic:
lower and duller); a grinding noise band moves up with the rate; sparks are sparse bright
crackle; the recorded loop plays underneath at 0.6.

## Horns (`../horn.ts`)

`disc_horn`: per horn a `PeriodicWave` with odd and even harmonics (the diaphragm slapping the
pole piece is asymmetric) through an asymmetric soft clip, a 2.4 kHz + 3.6 kHz formant pair and
a 6.5 kHz low-pass; two horns (`f1`, `f2` = 420 / 500 Hz) beat against each other; 12 ms
attack, pitch settling from 4 % flat; 6.3 Hz × 0.25 % vibrato. `air_horn`: sawtooth-like reed
per trumpet (`f1`, `f2`, `f3`: truck 220 / 277 / 330 Hz, police 311 / 370 Hz), 1.25 / 2.9 kHz
formants, breath noise, 45 ms attack from 10 % flat. The stop is the voice's release
(`release_s` 0.03). `load` = damage (front end / battery): up to −8 dB, a 17–23 Hz chatter that
chops the level between 1 − 0.6·d and 1, and the second horn sagging up to 90 cents flat.

## Checks

`node e2e/audio_impacts.mjs` (see its header) — renders in `docs/media/audio/impacts/`.
