"""Generate data/audio/LICENSES.md from data/audio/sources.json."""
import json, os, sys
A = sys.argv[1]
rows = json.load(open(os.path.join(A, "sources.json")))
L = {"CC0 1.0": "https://creativecommons.org/publicdomain/zero/1.0/", "CC-BY 3.0": "https://creativecommons.org/licenses/by/3.0/",
     "CC-BY 4.0": "https://creativecommons.org/licenses/by/4.0/"}
out = ["# Audio sample licences", "",
"Every recorded file under `data/audio/packs/` is listed below with its source page, author,",
"licence and what was done to it. Only CC0 1.0 and CC-BY (3.0/4.0) material is allowed — no NC, ND,",
"SA or ripped game audio. `cargo test -p wp_audio --test samples` fails if a referenced file is",
"missing from this table or carries another licence. Machine-readable copy: `sources.json`.",
"", "Processing (all files): converted to mono 44.1 kHz OGG Vorbis (q3 loops / q4 one-shots), silence",
"trimmed, loops crossfaded at the seam and normalised to the LUFS given, one-shots peak-normalised.",
"The tools that did it live in the integration doc (`docs/integration/audio.md`, \"Recorded packs\").", "",
"## Attribution (CC-BY)", ""]
by = {}
for r in rows:
    if r["licence"].startswith("CC-BY"):
        by.setdefault((r["author"], r["title"], r["page"], r["licence"]), []).append(r["file"])
for (a, t, p, l), fs in by.items():
    out.append(f"- **\"{t}\"** by **{a}** — {p} — licensed [{l}]({L.get(l, '')}). Modified (see table). Used in: {', '.join('`'+f+'`' for f in fs)}.")
out += ["", "These credits are also shown in the testbed's F1 help overlay.", "", "## Sources", ""]
seen = {}
for r in rows:
    seen.setdefault(r["page"], r)
for p, r in seen.items():
    out.append(f"- [{r['title']}]({p}) — {r['author']} — {r['licence']}")
out.append("- [Crickets Ambient Noise - loopable](https://opengameart.org/content/crickets-ambient-noise-loopable) — Wolfgang_ — CC0 1.0 (mixed into `city_night.ogg`)")
out += ["", "## Files", "", "| File | Licence | Author | Source | Original file | Processing |", "|---|---|---|---|---|---|"]
for r in rows:
    lic = r["licence"] + (" + " + r["also"]["licence"] if "also" in r else "")
    auth = r["author"] + (" + " + r["also"]["author"] if "also" in r else "")
    out.append(f"| {r['file']} | {lic} | {auth} | [{r['title']}]({r['page']}) | {r['original']} | {r['processing']} |")
out += ["", "## Licence texts", "",
"- CC0 1.0 Universal (public domain dedication): https://creativecommons.org/publicdomain/zero/1.0/legalcode — the Kenney pack ships it as `License.txt` (\"Creative Commons Zero, CC0 ... free to use in personal, educational and commercial projects\"); AntumDeluge's Scrapes ships the full CC0 1.0 legal code.",
"- CC-BY 3.0 Unported: https://creativecommons.org/licenses/by/3.0/legalcode — use, adapt and redistribute, including commercially, with attribution and an indication of changes (given above).",
"- CC-BY 4.0 International: https://creativecommons.org/licenses/by/4.0/legalcode — same conditions as CC-BY 3.0 (attribution, link to the licence, indication of changes; given above).",
"- Police sirens (`packs/siren_rec/`): two Wikimedia Commons recordings, licences checked on the file pages (2026-10-20). The wail / yelp / phaser recording is self-published by Heggyhomolit under CC0 (`{{self|Cc-zero}}`, dated 2011-01-07); its page is linked by page id because its title is a siren maker's trade name. The two-tone is Work With Sounds' recording of a police car's two-tone siren (Essen police, c. 2010; sound recordists Konrad Gutkowski, Jonathan Nicolai, Anna Pietzuch), CC-BY 4.0 with a Wikimedia permission ticket (2014082510012677).",
"- Some OpenGameArt entries are multi-licensed (e.g. CC-BY 3.0 / GPL); we use them under CC-BY 3.0 only.",
"- `engine_rec_i6` reuses the `engine_rec_v6` files and `engine_rec_turbo` the `engine_rec_i4` files (relative paths), so they carry the same licences.",
"- The tire squeal loop is qubodup's edit of audible-edge's freesound recording #71739; the licence given is the one on the OpenGameArt page (freesound.org was not reachable to cross-check)."]
open(os.path.join(A, "LICENSES.md"), "w").write("\n".join(out) + "\n")
print("ok", len(rows))
