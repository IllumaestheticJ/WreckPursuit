# tools/audio-packs — recorded sample packs from CC0 sources

Builds `data/audio/packs/<pack>.json` + `data/audio/packs/<pack>/*.ogg` from a manifest that
lists every source (URL, author, what it is) and the cuts taken from it, so a shipped sound can
always be traced back to its original page and rebuilt.

```
for p in world_rec engine_rec_i4 engine_rec_v8 engine_rec_i6 siren_rec radio_rec; do
  python3 tools/audio-packs/build.py tools/audio-packs/$p.json
done
```

Needs `ffmpeg` (with libvorbis) and `numpy`. Downloads are cached in `out/audio-cache/`
(gitignored). Freesound rate-limits bursts with HTTP 403; the script retries with back-off.

## Manifest

```jsonc
{
  "pack": "world_rec",          // output pack id
  "base": "world_proc",         // pack copied for every cue the manifest does not replace;
                                // also becomes `fallback_pack` (played while files load)
  "description": "…",
  "sources": {
    "fs237375": { "kind": "freesound", "id": 237375, "uid": 4144226, "author": "…", "what": "car crash" },
    "kenney_impact": { "kind": "zip", "url": "https://…/kenney_impact-sounds.zip", "author": "Kenney", "page": "…" }
  },
  "cues": {
    "impact/structural_crunch": {
      "gain": 1.0,              // cue gain in the pack
      "loop": false,            // true → seamless loop (equal-power tail→head crossfade)
      "variants": [ { "src": "fs237375", "start": 0.38, "end": 1.9 } ]   // seconds; zip sources add "file"
    }
  }
}
```

**Engine packs** add `"engine": { "cylinders": 4, "redline_rpm": 6500 }` (of the *recorded*
engine — the director maps layers to each car by `RedlineFrac = rpm / redline_rpm`) and
`"layers"`, which replace the base pack's layers:

```jsonc
"layers": [
  { "cue": "engine/idle", "idle": true, "rpm": 1230, "throttle": "Both", "gain": 0.7,
    "variants": [ { "src": "fs141240", "start": 57.5, "end": 63.5 } ] },
  { "cue": "engine/on_4500", "rpm": 4500, "throttle": "On", "gain": 0.95, "variants": [ … ] }
]
```

Pick each cut where the recording passes near the layer's rpm (within about ±40 %; a held rev is
best, a slow ramp works): the firing frequency (`rpm / 60 × cylinders / 2`) is tracked and the cut
resampled to exactly the layer's rpm. On layers come from pulls/held revs, off layers from
closed-throttle drops. A cue variant with `"rpm"` (and optional `"chop_hz"`) does the same for
`engine/limiter`: flatten to the redline, then gate like a fuel cut.

**Whistle / whine / filtering** (any variant): `"track_hz": [lo, hi]` + `"flatten_hz"` tracks the
strongest tone in that range and resamples it to a constant pitch (turbo whistle);
`"resample": r` shifts pitch by a constant ratio; `"band_hz": [lo, hi]` + `"air"` keeps only that
band (raised-cosine edges) plus broadband content above 2 kHz at `air` — isolates a whistle or
whine from the engine under it; `"lowpass_hz"` darkens (blow-off from air releases), `"highpass_hz"` strips rumble.
Applied in that order, before looping / one-shot shaping. A variant can instead be
`{ "join": [ {src, start, end}, … ], "xfade_s"? }`: short cuts (e.g. drag strokes) are RMS-matched
and joined with equal-power crossfades, then looped — longer than any single stroke. A cue's `"root_speed_mps"` is copied
into the pack (surface loops: the speed the recording was made at; pitch = speed / root).

**Siren loops** use `{ "src", "start", "cycles", "cycle_s", "xfade_s"? }`: `start` is phase 0 of
the cadence (sweep: the lowest pitch; hi-lo: the start of the high tone), the loop is exactly
`cycles × cycle_s` long. Put the same `cycle_s` (and the measured lo/hi/rise) in
`data/audio/audio_config.json`, because the backend starts the loop at `phase × cycle_s`.

Freesound sources use the site's HQ preview (`cdn.freesound.org/previews/…-hq.ogg`, no API key
needed). Only take sources whose page says **CC0** (search with the licence filter
`license:"Creative Commons 0"` and check the page). Never put brand or model names in `what`
(repo rule 9): describe the sound, not the car.

## Processing

- One-shots: 35 Hz high-pass, pre-roll trimmed to 5 ms before the attack, 3 ms fade-in,
  cosine fade over the last 25 %, peak −1 dBFS. `duration_s` = longest variant, rounded up
  to 50 ms.
- Engine layers: as loops, but the length is a whole number of engine cycles (720°) after
  flattening, with a 120 ms in-phase crossfade.
- Sirens: whole cycles, 20 ms crossfade (10 ms for the fast sweep).
- Loops: 35 Hz high-pass, 250 ms (or ¼ length) equal-power crossfade of the tail into the head,
  RMS −20 dBFS, peak capped at −1 dBFS.
- 48 kHz mono Ogg Vorbis q6, metadata stripped.

The generated `CREDITS.md` next to the files lists every source, author and licence.

## Known gaps

- Cylinder counts and rpm of the recorded engines are inferred from the tracked firing
  frequency and plausible idle speeds; the six may in fact be a V8 (only the layer spacing
  depends on it).
- Surface `root_speed_mps` values are estimates; the recordings carry no speed data.
- Freesound blocks the IP for ~40 min after a burst of requests; keep searches ≥ 25 s apart.
- Cuts were chosen from onset/decay/pitch analysis, not by ear; retune `start`/`end`/`gain` after
  listening (rebuild is ~10 s).
- Safari before 17 cannot decode Ogg Vorbis; the testbed then plays the fallback synths.
