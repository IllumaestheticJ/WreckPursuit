# Recorded police sirens integration notes (owner: "police sirens also seem unrealistic — use real sounds")

- **Workstream:** lead helper, recorded sirens
- **Branch:** `port/round12-sirens` (from `origin/lead/round12` @ 3cd43db7)
- **Files owned:** `data/audio/packs/siren_rec.json`, `data/audio/packs/siren_rec/*.ogg`, the siren
  rows of `data/audio/sources.json` / `LICENSES.md`, the sample-siren playback path in
  `apps/testbed-web/src/audio/voices.ts` (`sirenLoopVariation`, `oggVorbisLength`),
  `apps/testbed-web/e2e/siren_samples.mjs`, `crates/wp_audio/examples/siren_trace.rs`
- **Contract version built against:** 0.3.0 (no contract change; the simulation is untouched)

## What was wrong

1. **The game played the synth.** `sentinel_{bastion,cruiser,wolfhound}` and the director's
   default siren pack were `siren_v2` (playtest row 82: the synthesized horn-speaker siren);
   `siren_rec` was shipped but nothing selected it. Every siren the owner heard was synthesized.
2. **`siren_rec` itself was poor:** all four loops came from one file; the wail was a rising sweep
   plus its time-reversed copy, the hi-lo a steady tone re-pitched to 960 / 770 Hz, the yelp two
   cycles, the phaser eight. Measured through the real path: wail 7.0 s per "cycle" at
   722–1046 Hz, no clear phaser period.
3. **Every recorded loop dropped out at its wrap in the browser** (found while validating):
   Chromium's `decodeAudioData` ignores the Ogg Vorbis end trim (last page's granule position)
   and returns the whole last block, 52–919 samples (up to ~21 ms) of decaying codec tail past
   the real end. Looped, that is a dip of −25 dB (wail) … −46 dB (yelp) at every wrap. It applies
   to every `.ogg` loop (engine idle: +846 samples), not only sirens.
4. **A recorded siren's Doppler stuck after a pass-by** (found the same way): `VoicePool`
   recorded the pitch as applied on every phase change, and a siren's phase changes every
   frame, so slow pitch changes never reached `playbackRate` — receding sirens stayed at ×0.98
   instead of ×0.93. The synth was unaffected (it reads every frame).

## What changed

- **Recordings** (both Wikimedia Commons, licences read on the file pages 2026-10-20; details,
  page links and processing per file in `data/audio/LICENSES.md` / `sources.json`):
  - wail, yelp, phaser: an electronic siren on a fire apparatus, CC0 (Heggyhomolit,
    https://commons.wikimedia.org/w/index.php?curid=12624440 — linked by page id because the
    file title is a siren maker's trade name);
  - hi-lo: a German police car's two-tone (Essen police, c. 2010), CC-BY 4.0 (Work With
    Sounds / Konrad Gutkowski, https://commons.wikimedia.org/wiki/File:WWS_Policecarsiren.ogg),
    credited in the F1 help (`credits.ts`).
- **Loops** (`tools/audio_packs/build_packs.py`, siren section; reproduces the files bit for
  bit): whole natural cycles, nothing reversed, re-pitched or synthesized — one wail cycle
  (4.73 s: rise 860 → 1660 Hz in ~2 s, slow fall), seven yelp cycles (331 ms each, 860–1630 Hz),
  one whole fast-phaser pattern (~75 ms sweeps, 840–1200 Hz, repeating every 1.040 s), six
  two-tone cycles (457 Hz 0.72 s / 603 Hz 0.74 s). Start on a rising zero crossing, end matched
  by cross-correlation (r 0.97–0.99), 6–12 ms seam crossfade with the audio that follows the
  end in the recording; high-pass 200 / 120 Hz (mains hum), mono, −16 LUFS, OGG q3; 128 KB
  total. The source has only ~2.8 s of yelp and ~1.4 s of phaser, so those loops are as long as
  the recording allows; an electronic siren's controller repeats exactly, so a longer loop would
  not sound different.
- **Pack** `siren_rec` (`tools/audio_packs/write_packs.py`): the four recorded loops (gain 2.371
  = +7.5 dB, which puts them at the synth's loudness on the Police bus, parked at 30 m: wail and
  yelp −27.1 vs −27.2 LUFS, phaser +0.7 LU, hi-lo −2.1 LU),
  `release_s` 0.15 like `siren_v2`; whoop and air horn stay the `siren_v2` synth cues (no
  permissively licensed recording found; the Commons air-horn clips are CC BY-SA); fallback
  `siren_v2` (procedural mode, and while the files decode).
- **Selection:** police car defs → `siren_rec`; `DEFAULT_SIREN_PACK` (`director.rs`) and the
  fallback police def (`data.rs`) → `siren_rec`. Guard: `tests/samples.rs` fails if a car's siren
  pack plays no recordings.
- **Sample-siren playback** (`voices.ts`): `sirenLoopVariation` — a recorded loop plays at its
  unit's rate (the director's per-unit pitch spread, `synth.lo_hz` / configured `lo_hz`,
  ±1.5 %) from its unit's point in the loop (`synth.unit`), so several units beat and never
  start in lockstep, like the synth; `oggVorbisLength` + trim after decode (fixes 3 for every
  pack); `l.pitch` only advances when applied (fixes 4).
- Already in place and unchanged, now applied to the recordings: distance attenuation, air
  absorption low-pass, Doppler on the playback rate, HRTF panning, occlusion, street slap echo,
  tunnel reverb, interior muffle, engine / radio ducks (`siren_space.rs`, `siren_rack.ts`). No
  extra horn filtering: the recordings carry the speaker.

## Exposes

- `apps/testbed-web/src/audio/voices.ts`: `sirenLoopVariation(lib, pack, cue, synth)`,
  `oggVorbisLength(bytes)`.
- `cargo run -p wp_audio --example siren_trace -- out.json`: the director's siren stream for a
  police cruiser (wail / yelp / phaser / hi-lo at 30 m, hi-lo pass-by at 27 m/s).
- `node apps/testbed-web/e2e/siren_samples.mjs [--trace f] [--source recorded|procedural]
  [--pack p] [--wav dir] [--label l]` (`npm run e2e:siren-samples`): renders that stream
  through the real `VoicePool` + `SirenRack` (Police bus only) and checks recordings play (no
  synth), cadence (wail 4–6 s, yelp 2.5–4 /s, phaser > 8 /s, hi-lo 0.5–0.8 s per tone), loop
  seams (no HF burst, no level dip at any wrap), Doppler (×1.085 / ×0.929 measured vs ×1.085 /
  ×0.927 model) and distance level.

## Changes outside my files (minimal)

- `crates/wp_audio/src/director.rs` (`DEFAULT_SIREN_PACK`), `crates/wp_audio/src/data.rs` (the
  fallback police def's siren pack): one line each.
- `data/audio/cars/sentinel_*.json`: `siren.pack`. `apps/testbed-web/package.json`: script.
- Not touched: `synth.ts` `siren()`, `siren.rs` (`freq_at_phase`, tests), `siren_unit.rs`,
  `siren_space.rs`, `siren_rack.ts`, `siren_lab.ts` (the lab still auditions the synth).

## Perf numbers

| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| Sample siren voice: `sirenLoopVariation` per Play, trim once per decode | headless Chromium | — | not measurable in the audio frame (`e2e/audio.mjs` 24-car bench unchanged: 2.1 ms) | 0.5 ms police |

## Known gaps

- Whoop and air horn are still synthesized (no CC0 / CC-BY recording found).
- One siren timbre for every unit (one recording per pattern); the per-unit rate spread and start
  offset keep units apart, but they share a speaker.
- The two-tone is a European one (a fourth, 0.72 / 0.74 s per tone, the device's real timing);
  it plays when `sirens.v2.two_tone` is set or hi-lo is forced.
- The siren lab (audio panel) auditions the synth only.

## Lead feedback
