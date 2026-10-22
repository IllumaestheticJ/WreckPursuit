# Audio sample licences

Every recorded file under `data/audio/packs/` is listed below with its source page, author,
licence and what was done to it. Only CC0 1.0 and CC-BY (3.0/4.0) material is allowed — no NC, ND,
SA or ripped game audio. `cargo test -p wp_audio --test samples` fails if a referenced file is
missing from this table or carries another licence. Machine-readable copy: `sources.json`.

Processing (all files): converted to mono 44.1 kHz OGG Vorbis (q3 loops / q4 one-shots), silence
trimmed, loops crossfaded at the seam and normalised to the LUFS given, one-shots peak-normalised.
The tools that did it live in the integration doc (`docs/integration/audio.md`, "Recorded packs").

## Attribution (CC-BY)

- **"Car Engine Loop 96kHz, 4s"** by **qubodup** — https://opengameart.org/content/car-engine-loop-96khz-4s — licensed [CC-BY 3.0](https://creativecommons.org/licenses/by/3.0/). Modified (see table). Used in: `packs/engine_rec_i4/mid_on.ogg`, `packs/engine_rec_i4/mid_off.ogg`.
- **"Engine-loop heavy vehicle/tank"** by **Nayckron** — https://opengameart.org/content/engine-loop-heavy-vehicletank — licensed [CC-BY 3.0](https://creativecommons.org/licenses/by/3.0/). Modified (see table). Used in: `packs/engine_rec_v8/idle.ogg`, `packs/engine_rec_v8/low_on.ogg`, `packs/engine_rec_v8/mid_on.ogg`, `packs/engine_rec_v8/high_on.ogg`, `packs/engine_rec_v8/low_off.ogg`, `packs/engine_rec_v8/high_off.ogg`.
- **"Car tire squeal skid loop"** by **qubodup (edit of audible-edge, freesound #71739)** — https://opengameart.org/content/car-tire-squeal-skid-loop — licensed [CC-BY 3.0](https://creativecommons.org/licenses/by/3.0/). Modified (see table). Used in: `packs/world_rec/squeal.ogg`.

These credits are also shown in the testbed's F1 help overlay.

## Sources

- [Car Sound Effects Pack (Low Quality)](https://opengameart.org/content/car-sound-effects-pack-low-quality) — GGBotNet — CC0 1.0
- [Car Engine Loop 96kHz, 4s](https://opengameart.org/content/car-engine-loop-96khz-4s) — qubodup — CC-BY 3.0
- [racing car engine sound loops](https://opengameart.org/content/racing-car-engine-sound-loops) — domasx2 (from a public-domain pdsounds.org recording) — CC0 1.0
- [Engine-loop heavy vehicle/tank](https://opengameart.org/content/engine-loop-heavy-vehicletank) — Nayckron — CC-BY 3.0
- [Steam release sounds](https://opengameart.org/content/steam-release-sounds) — bart — CC0 1.0
- [Car tire squeal skid loop](https://opengameart.org/content/car-tire-squeal-skid-loop) — qubodup (edit of audible-edge, freesound #71739) — CC-BY 3.0
- [Some sounds](https://opengameart.org/content/some-sounds-0) — Ziph — CC0 1.0
- [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) — Kenney (www.kenney.nl) — CC0 1.0
- [Crash Collision](https://opengameart.org/content/crash-collision) — qubodup — CC0 1.0
- [Glass Break](https://opengameart.org/content/glass-break) — TinyWorlds (Till Behrend) — CC0 1.0
- [75 CC0 breaking / falling / hit sfx](https://opengameart.org/content/75-cc0-breaking-falling-hit-sfx) — rubberduck — CC0 1.0
- [Mechanical Sounds](https://opengameart.org/content/mechanical-sounds) — BMacZero — CC0 1.0
- [Scrapes](https://opengameart.org/content/scrapes) — AntumDeluge — CC0 1.0
- [High traffic road sounds](https://opengameart.org/content/high-traffic-road-sounds) — IgnasD — CC0 1.0
- [Sirens and Alarm Noise](https://opengameart.org/content/sirens-and-alarm-noise) — aquinn — CC0 1.0
- [Radio Noise 1](https://opengameart.org/content/radio-noise-1) — Kresiek The Furry — CC0 1.0
- [Crickets Ambient Noise - loopable](https://opengameart.org/content/crickets-ambient-noise-loopable) — Wolfgang_ — CC0 1.0 (mixed into `city_night.ogg`)

## Files

| File | Licence | Author | Source | Original file | Processing |
|---|---|---|---|---|---|
| packs/engine_rec_i4/idle.ogg | CC0 1.0 | GGBotNet | [Car Sound Effects Pack (Low Quality)](https://opengameart.org/content/car-sound-effects-pack-low-quality) | Car_Engine_Loop_2.ogg | mono, crossfade loop 0.08s, -20 LUFS |
| packs/engine_rec_i4/mid_on.ogg | CC-BY 3.0 | qubodup | [Car Engine Loop 96kHz, 4s](https://opengameart.org/content/car-engine-loop-96khz-4s) | engine-loop-1.wav | mono (96 kHz → 44.1 kHz), crossfade loop 0.08s, -20 LUFS |
| packs/engine_rec_i4/high_on.ogg | CC0 1.0 | GGBotNet | [Car Sound Effects Pack (Low Quality)](https://opengameart.org/content/car-sound-effects-pack-low-quality) | Car_Engine_Loop.ogg | mono, crossfade loop 0.08s, -20 LUFS |
| packs/engine_rec_i4/mid_off.ogg | CC-BY 3.0 | qubodup | [Car Engine Loop 96kHz, 4s](https://opengameart.org/content/car-engine-loop-96khz-4s) | engine-loop-1.wav | mono, low-pass 900 Hz (overrun layer), crossfade loop 0.08s, -24 LUFS |
| packs/engine_rec_i4/high_off.ogg | CC0 1.0 | GGBotNet | [Car Sound Effects Pack (Low Quality)](https://opengameart.org/content/car-sound-effects-pack-low-quality) | Car_Engine_Loop.ogg | mono, low-pass 1.2 kHz (overrun layer), crossfade loop 0.08s, -24 LUFS |
| packs/engine_rec_v6/idle.ogg | CC0 1.0 | domasx2 (from a public-domain pdsounds.org recording) | [racing car engine sound loops](https://opengameart.org/content/racing-car-engine-sound-loops) | loop_0.wav | mono, crossfade loop 0.02s, -20 LUFS |
| packs/engine_rec_v6/low_on.ogg | CC0 1.0 | domasx2 (from a public-domain pdsounds.org recording) | [racing car engine sound loops](https://opengameart.org/content/racing-car-engine-sound-loops) | loop_2_0.wav | mono, crossfade loop 0.02s, -20 LUFS |
| packs/engine_rec_v6/mid_on.ogg | CC0 1.0 | domasx2 (from a public-domain pdsounds.org recording) | [racing car engine sound loops](https://opengameart.org/content/racing-car-engine-sound-loops) | loop_4_0.wav | mono, crossfade loop 0.02s, -20 LUFS |
| packs/engine_rec_v6/high_on.ogg | CC0 1.0 | domasx2 (from a public-domain pdsounds.org recording) | [racing car engine sound loops](https://opengameart.org/content/racing-car-engine-sound-loops) | loop_5_0.wav | mono, crossfade loop 0.02s, -20 LUFS |
| packs/engine_rec_v6/low_off.ogg | CC0 1.0 | domasx2 (from a public-domain pdsounds.org recording) | [racing car engine sound loops](https://opengameart.org/content/racing-car-engine-sound-loops) | loop_1_0.wav | mono, low-pass 700 Hz (overrun layer), crossfade loop 0.02s, -24 LUFS |
| packs/engine_rec_v6/high_off.ogg | CC0 1.0 | domasx2 (from a public-domain pdsounds.org recording) | [racing car engine sound loops](https://opengameart.org/content/racing-car-engine-sound-loops) | loop_3_0.wav | mono, low-pass 900 Hz (overrun layer), crossfade loop 0.02s, -24 LUFS |
| packs/engine_rec_v8/idle.ogg | CC-BY 3.0 | Nayckron | [Engine-loop heavy vehicle/tank](https://opengameart.org/content/engine-loop-heavy-vehicletank) | engine_heavy_slow_loop.wav | mono, crossfade loop 0.08s, -20 LUFS |
| packs/engine_rec_v8/low_on.ogg | CC-BY 3.0 | Nayckron | [Engine-loop heavy vehicle/tank](https://opengameart.org/content/engine-loop-heavy-vehicletank) | engine_heavy_loop.wav | mono, crossfade loop 0.08s, -20 LUFS |
| packs/engine_rec_v8/mid_on.ogg | CC-BY 3.0 | Nayckron | [Engine-loop heavy vehicle/tank](https://opengameart.org/content/engine-loop-heavy-vehicletank) | engine_heavy_average_loop.wav | mono, crossfade loop 0.08s, -20 LUFS |
| packs/engine_rec_v8/high_on.ogg | CC-BY 3.0 | Nayckron | [Engine-loop heavy vehicle/tank](https://opengameart.org/content/engine-loop-heavy-vehicletank) | engine_heavy_fast_loop.wav | mono, crossfade loop 0.08s, -20 LUFS |
| packs/engine_rec_v8/low_off.ogg | CC-BY 3.0 | Nayckron | [Engine-loop heavy vehicle/tank](https://opengameart.org/content/engine-loop-heavy-vehicletank) | engine_heavy_loop.wav | mono, low-pass 500 Hz (overrun layer), crossfade loop 0.08s, -24 LUFS |
| packs/engine_rec_v8/high_off.ogg | CC-BY 3.0 | Nayckron | [Engine-loop heavy vehicle/tank](https://opengameart.org/content/engine-loop-heavy-vehicletank) | engine_heavy_average_loop.wav | mono, low-pass 700 Hz (overrun layer), crossfade loop 0.08s, -24 LUFS |
| packs/engine_rec_turbo/blowoff_1.ogg | CC0 1.0 | bart | [Steam release sounds](https://opengameart.org/content/steam-release-sounds) | steam hisses - Marker #1.wav | mono, first 0.6 s, exponential decay, peak −1 dBFS |
| packs/engine_rec_turbo/blowoff_2.ogg | CC0 1.0 | bart | [Steam release sounds](https://opengameart.org/content/steam-release-sounds) | steam hisses - Marker #3.wav | mono, first 0.6 s, exponential decay, peak −1 dBFS |
| packs/engine_rec_turbo/blowoff_3.ogg | CC0 1.0 | bart | [Steam release sounds](https://opengameart.org/content/steam-release-sounds) | steam hisses - Marker #4.wav | mono, first 0.6 s, exponential decay, peak −1 dBFS |
| packs/world_rec/squeal.ogg | CC-BY 3.0 | qubodup (edit of audible-edge, freesound #71739) | [Car tire squeal skid loop](https://opengameart.org/content/car-tire-squeal-skid-loop) | tires_squal_loop.wav | mono (96 kHz → 44.1 kHz), crossfade loop 0.05s, -20 LUFS |
| packs/world_rec/shift_1.ogg | CC0 1.0 | Ziph | [Some sounds](https://opengameart.org/content/some-sounds-0) | gear01.wav | mono, trimmed to the clunk, peak −1 dBFS |
| packs/world_rec/shift_2.ogg | CC0 1.0 | Ziph | [Some sounds](https://opengameart.org/content/some-sounds-0) | gear04.wav | mono, trimmed to the clunk, peak −1 dBFS |
| packs/world_rec/shift_3.ogg | CC0 1.0 | Ziph | [Some sounds](https://opengameart.org/content/some-sounds-0) | gear06.wav | mono, trimmed to the clunk, peak −1 dBFS |
| packs/world_rec/shift_4.ogg | CC0 1.0 | Ziph | [Some sounds](https://opengameart.org/content/some-sounds-0) | gear07.wav | mono, trimmed to the clunk, peak −1 dBFS |
| packs/world_rec/thud_1.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactSoft_heavy_000.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/land_1.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactSoft_heavy_002.ogg | mono, trimmed, peak −1 dBFS, pitch ×0.8 |
| packs/world_rec/thump_1.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactSoft_medium_000.ogg | mono, trimmed, peak −1 dBFS, pitch ×0.85 |
| packs/world_rec/thud_2.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactSoft_heavy_001.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/land_2.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactSoft_heavy_003.ogg | mono, trimmed, peak −1 dBFS, pitch ×0.8 |
| packs/world_rec/thump_2.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactSoft_medium_001.ogg | mono, trimmed, peak −1 dBFS, pitch ×0.85 |
| packs/world_rec/thud_3.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactSoft_heavy_002.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/land_3.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactSoft_heavy_004.ogg | mono, trimmed, peak −1 dBFS, pitch ×0.8 |
| packs/world_rec/thump_3.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactSoft_medium_002.ogg | mono, trimmed, peak −1 dBFS, pitch ×0.85 |
| packs/world_rec/plastic_1.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactGeneric_light_000.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/metal_light_1.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactMetal_light_000.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/metal_medium_1.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactMetal_medium_000.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/metal_heavy_1.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactMetal_heavy_000.ogg | mono, trimmed, peak −1 dBFS, pitch ×0.8 |
| packs/world_rec/glass_1.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactGlass_heavy_000.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/clank_1.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactPlate_heavy_000.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/plastic_2.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactGeneric_light_001.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/metal_light_2.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactMetal_light_001.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/metal_medium_2.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactMetal_medium_001.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/metal_heavy_2.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactMetal_heavy_001.ogg | mono, trimmed, peak −1 dBFS, pitch ×0.8 |
| packs/world_rec/glass_2.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactGlass_heavy_001.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/clank_2.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactPlate_heavy_001.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/plastic_3.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactGeneric_light_002.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/metal_light_3.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactMetal_light_002.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/metal_medium_3.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactMetal_medium_002.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/metal_heavy_3.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactMetal_heavy_002.ogg | mono, trimmed, peak −1 dBFS, pitch ×0.8 |
| packs/world_rec/glass_3.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactGlass_heavy_002.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/clank_3.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactPlate_heavy_002.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/plastic_4.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactGeneric_light_003.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/metal_light_4.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactMetal_light_003.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/metal_medium_4.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactMetal_medium_003.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/metal_heavy_4.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactMetal_heavy_003.ogg | mono, trimmed, peak −1 dBFS, pitch ×0.8 |
| packs/world_rec/glass_4.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactGlass_heavy_003.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/clank_4.ogg | CC0 1.0 | Kenney (www.kenney.nl) | [Impact Sounds (1.0)](https://kenney.nl/assets/impact-sounds) | impactPlate_heavy_003.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/metal_heavy_5.ogg | CC0 1.0 | qubodup | [Crash Collision](https://opengameart.org/content/crash-collision) | qubodup-crash.ogg | mono, trimmed, peak −1 dBFS, pitch ×0.75 |
| packs/world_rec/glass_5.ogg | CC0 1.0 | TinyWorlds (Till Behrend) | [Glass Break](https://opengameart.org/content/glass-break) | glass_breaking.wav | mono, trimmed, peak −1 dBFS |
| packs/world_rec/glass_6.ogg | CC0 1.0 | rubberduck | [75 CC0 breaking / falling / hit sfx](https://opengameart.org/content/75-cc0-breaking-falling-hit-sfx) | bfh1_glass_breaking_01.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/glass_7.ogg | CC0 1.0 | rubberduck | [75 CC0 breaking / falling / hit sfx](https://opengameart.org/content/75-cc0-breaking-falling-hit-sfx) | bfh1_glass_breaking_03.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/part_tear_1.ogg | CC0 1.0 | rubberduck | [75 CC0 breaking / falling / hit sfx](https://opengameart.org/content/75-cc0-breaking-falling-hit-sfx) | bfh1_metal_falling_01.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/part_tear_2.ogg | CC0 1.0 | rubberduck | [75 CC0 breaking / falling / hit sfx](https://opengameart.org/content/75-cc0-breaking-falling-hit-sfx) | bfh1_metal_falling_02.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/wheel_off_1.ogg | CC0 1.0 | rubberduck | [75 CC0 breaking / falling / hit sfx](https://opengameart.org/content/75-cc0-breaking-falling-hit-sfx) | bfh1_falling_01.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/wheel_off_2.ogg | CC0 1.0 | rubberduck | [75 CC0 breaking / falling / hit sfx](https://opengameart.org/content/75-cc0-breaking-falling-hit-sfx) | bfh1_falling_07.ogg | mono, trimmed, peak −1 dBFS |
| packs/world_rec/engine_fail.ogg | CC0 1.0 | BMacZero | [Mechanical Sounds](https://opengameart.org/content/mechanical-sounds) | clank1.wav | mono, trimmed, peak −1 dBFS, pitch ×0.7 |
| packs/world_rec/scrape_metal.ogg | CC0 1.0 | AntumDeluge | [Scrapes](https://opengameart.org/content/scrapes) | scrape-1/3/5/7.wav | mono, 4 takes joined, crossfade loop 0.1s, -20 LUFS |
| packs/world_rec/scrape_plastic.ogg | CC0 1.0 | AntumDeluge | [Scrapes](https://opengameart.org/content/scrapes) | scrape-2/4/6/8.wav | mono, 4 takes joined, pitch ×1.5, crossfade loop 0.1s, -22 LUFS |
| packs/world_rec/steam_hiss.ogg | CC0 1.0 | bart | [Steam release sounds](https://opengameart.org/content/steam-release-sounds) | steam hisses - Marker #2.wav | mono, sustained middle part, crossfade loop 0.2s, -24 LUFS |
| packs/world_rec/horn.ogg | CC0 1.0 | GGBotNet | [Car Sound Effects Pack (Low Quality)](https://opengameart.org/content/car-sound-effects-pack-low-quality) | Car_Horn.ogg | mono, loud body of the honk (from the 0.08–0.40 s cut), envelope flattened, autocorrelation loop 32 ms tiled to 0.51 s (horn_sustain.py), peak -6 dBFS |
| packs/world_rec/horn_truck.ogg | CC0 1.0 | GGBotNet | [Car Sound Effects Pack (Low Quality)](https://opengameart.org/content/car-sound-effects-pack-low-quality) | Car_Horn.ogg | mono, pitch ×0.7, loud body of the honk, envelope flattened, autocorrelation loop 46 ms tiled to 0.51 s (horn_sustain.py), peak -6 dBFS |
| packs/world_rec/city_night.ogg | CC0 1.0 + CC0 1.0 | IgnasD + Wolfgang_ | [High traffic road sounds](https://opengameart.org/content/high-traffic-road-sounds) | gatve Varniu_2.ogg + crickets_1.mp3 | mono, 40 s of distant traffic (low-pass 2.5 kHz) + crickets (Wolfgang_, CC0) at −12 dB, crossfade loop 1.5s, -26 LUFS |
| packs/siren_rec/wail.ogg | CC0 1.0 | aquinn | [Sirens and Alarm Noise](https://opengameart.org/content/sirens-and-alarm-noise) | siren_0.mp3 | mono, rising sweep 26.50–30.26 s + its mirror (fall), crossfade loop 0.02s, -16 LUFS |
| packs/siren_rec/yelp.ogg | CC0 1.0 | aquinn | [Sirens and Alarm Noise](https://opengameart.org/content/sirens-and-alarm-noise) | siren_0.mp3 | mono, two 0.68 s yelp cycles 36.52–37.88 s, crossfade loop 0.02s, -16 LUFS |
| packs/siren_rec/phaser.ogg | CC0 1.0 | aquinn | [Sirens and Alarm Noise](https://opengameart.org/content/sirens-and-alarm-noise) | siren_0.mp3 | mono, eight 0.125 s cycles 9.08–10.08 s, crossfade loop 0.02s, -16 LUFS |
| packs/siren_rec/hilo.ogg | CC0 1.0 | aquinn | [Sirens and Alarm Noise](https://opengameart.org/content/sirens-and-alarm-noise) | siren_0.mp3 | mono, steady tone 29.90–30.20 s re-pitched to 960 / 770 Hz, 0.55 s each, crossfade loop 0.03s, -16 LUFS |
| packs/radio_rec/static.ogg | CC0 1.0 | Kresiek The Furry | [Radio Noise 1](https://opengameart.org/content/radio-noise-1) | radio_noise_loop.wav | mono, 10 s (8 kHz source), crossfade loop 0.3s, -24 LUFS |
| packs/radio_rec/squelch_on.ogg | CC0 1.0 | Kresiek The Furry | [Radio Noise 1](https://opengameart.org/content/radio-noise-1) | radio_noise_loop.wav | mono, 0.14s burst at 20.0s, exponential decay, peak −3 dBFS |
| packs/radio_rec/squelch_off.ogg | CC0 1.0 | Kresiek The Furry | [Radio Noise 1](https://opengameart.org/content/radio-noise-1) | radio_noise_loop.wav | mono, 0.28s burst at 31.0s, exponential decay, peak −3 dBFS |

## Licence texts

- CC0 1.0 Universal (public domain dedication): https://creativecommons.org/publicdomain/zero/1.0/legalcode — the Kenney pack ships it as `License.txt` ("Creative Commons Zero, CC0 ... free to use in personal, educational and commercial projects"); AntumDeluge's Scrapes ships the full CC0 1.0 legal code.
- CC-BY 3.0 Unported: https://creativecommons.org/licenses/by/3.0/legalcode — use, adapt and redistribute, including commercially, with attribution and an indication of changes (given above).
- Some OpenGameArt entries are multi-licensed (e.g. CC-BY 3.0 / GPL); we use them under CC-BY 3.0 only.
- `engine_rec_i6` reuses the `engine_rec_v6` files and `engine_rec_turbo` the `engine_rec_i4` files (relative paths), so they carry the same licences.
- The tire squeal loop is qubodup's edit of audible-edge's freesound recording #71739; the licence given is the one on the OpenGameArt page (freesound.org was not reachable to cross-check).
