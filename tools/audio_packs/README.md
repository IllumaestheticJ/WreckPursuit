# audio_packs — how the recorded packs in `data/audio/packs/*_rec*` were made

Not part of any build; kept so the files can be regenerated or extended. Needs `ffmpeg` (with
libvorbis) and Python 3 with `numpy`, `pyloudnorm` (`py7zr` to unpack `.7z` downloads).

1. `python fetch.py <dl> <opengameart slugs…>` — downloads each entry and its page metadata. The
   Kenney "Impact Sounds" zip is fetched by hand from https://kenney.nl/assets/impact-sounds and
   unzipped to `<dl>/kenney_impact/`. Unpack `.7z` files next to them (`<file>.7z_x/`).
   The siren recordings come from Wikimedia Commons (`upload.wikimedia.org`, rate-limited: fetch
   slowly) and are saved under neutral names in `<dl>/commons/`:
   `cc0_fire_wail_yelp_phaser.ogg` = https://commons.wikimedia.org/w/index.php?curid=12624440
   (CC0, Heggyhomolit) and `ccby_de_police_twotone.ogg` =
   https://commons.wikimedia.org/wiki/File:WWS_Policecarsiren.ogg (CC-BY 4.0, Work With Sounds /
   Konrad Gutkowski). The siren section needs `scipy` too.
2. `python build_packs.py <dl> data/audio` — cuts, loops, normalises and encodes every file, writes
   `data/audio/sources.json` (source page, author, licence, processing per file).
3. `python write_packs.py data/audio` — writes the `*_rec` pack JSONs from the procedural packs
   (synth cues are kept where no recording exists) and points the car defs at them.
4. `python licenses.py data/audio` — regenerates `data/audio/LICENSES.md`.
5. `cargo test -p wp_audio --test samples` and `node apps/testbed-web/e2e/audio_samples.mjs`
   (sirens: `node apps/testbed-web/e2e/siren_samples.mjs` — cadence, loop seams, Doppler).

Licence rule: CC0 or CC-BY only (read the page; multi-licensed entries are used under CC-BY).
Every new CC-BY author also goes into `apps/testbed-web/src/audio/credits.ts`.
