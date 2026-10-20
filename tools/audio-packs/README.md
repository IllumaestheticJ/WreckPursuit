# tools/audio-packs — recorded sample packs from CC0 sources

Builds `data/audio/packs/<pack>.json` + `data/audio/packs/<pack>/*.ogg` from a manifest that
lists every source (URL, author, what it is) and the cuts taken from it, so a shipped sound can
always be traced back to its original page and rebuilt.

```
python3 tools/audio-packs/build.py tools/audio-packs/world_rec.json
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

Freesound sources use the site's HQ preview (`cdn.freesound.org/previews/…-hq.ogg`, no API key
needed). Only take sources whose page says **CC0** (search with the licence filter
`license:"Creative Commons 0"` and check the page). Never put brand or model names in `what`
(repo rule 9): describe the sound, not the car.

## Processing

- One-shots: 35 Hz high-pass, pre-roll trimmed to 5 ms before the attack, 3 ms fade-in,
  cosine fade over the last 25 %, peak −1 dBFS. `duration_s` = longest variant, rounded up
  to 50 ms.
- Loops: 35 Hz high-pass, 250 ms (or ¼ length) equal-power crossfade of the tail into the head,
  RMS −20 dBFS, peak capped at −1 dBFS.
- 48 kHz mono Ogg Vorbis q6, metadata stripped.

The generated `CREDITS.md` next to the files lists every source, author and licence.

## Known gaps

- Cuts were chosen from onset/decay analysis, not by ear; retune `start`/`end`/`gain` after
  listening (rebuild is ~10 s).
- Safari before 17 cannot decode Ogg Vorbis; the testbed then plays the fallback synths.
