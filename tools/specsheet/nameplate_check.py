#!/usr/bin/env python3
"""Fail if any real car maker, model, badge or trademarked tech name appears in roster files.

The denylist itself would break the naming policy (CLAUDE.md rule 9: no licensed names
anywhere), so it is stored as truncated SHA-256 hashes of lower-case words. Every word and
every adjacent word pair (joined with a space), plus hyphenated runs with the hyphens
removed (so "AB-12" is also checked as "ab12"), in the scanned files is hashed and compared.

Two tiers:
- STRICT  — maker/model/badge/tech names that are never ordinary words: banned everywhere.
- NAME    — nameplates that are also ordinary English words ("charger", "golf", "ram"...):
            banned only where a name is being given — car `id`, `display_name`,
            `analog_of`, and Markdown headings in the scanned files.

Usage (repo root):
    python3 tools/specsheet/nameplate_check.py            # exit 1 on any hit
    python3 tools/specsheet/nameplate_check.py --hash "some name"   # print a hash to add

Sound-alike / one-letter-off names cannot be caught by hashing; review new names by hand
against the policy in data/cars/README.md.
"""

from __future__ import annotations

import glob
import hashlib
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", ".."))

# Files owned by the car-roster workstream.
SCAN = [
    "data/cars/*.json",
    "data/cars/*.md",
    "data/lattices/*.json",
    "tools/specsheet/*.py",
    "tools/specsheet/*.md",
    "docs/integration/cars.md",
]

STRICT = {
    "02f23038fc90ab7d", "06ba2682508e2347", "09b7b4bebb61f328", "09c587fb282c3423", "125dba6b005498e6",
    "13aa6127ef7bdca2", "14608a7308a0807f", "153812ae5fea0b73", "16103dd07c46aa4c", "1bf0b8108131f88b",
    "1c07b8c298928265", "1c319d594acddfd6", "1c39a01150f2883c", "1ed60b8439cc9844", "20d1a141586e97cc",
    "21e2f8144cc49649", "248f9a8dd8f1153f", "25243c29870a96e0", "27df9ed9a477af0f", "2b584134ddc9c672",
    "2d51a3b3ca1cdf79", "31482014fa7a5e51", "337b8d2c1e132acd", "35bab8adf7600be1", "363554e2e3d91266",
    "36b53ead480d4176", "37268bb002000746", "3763ca091d9c265b", "3ae83660f6760022", "3bc175e9ce56d1fc",
    "3fa05ffbbbef4e31", "3ff74a580f8b39f0", "3ff8bfcf00403a50", "40ba25ddfe4dad6b", "40fc6718a81fe76c",
    "4261d227519924d7", "44575b869e9dfb6a", "44dbc6bca68fed5a", "44e8dd36c54ce8a4", "48bf83a5fb5e9555",
    "4d13e1dbe7ab3f90", "4d4ea86ae0f5a5f8", "4defabfa3f9e9904", "53008a4021d50189", "5336996af49cb064",
    "559054694ada46f5", "55dd9a9539071689", "573abb3ca06e6abb", "576bb94858e6d3ef", "57fac8572eeab5fc",
    "59368d281b0bd775", "5afa39317c9f6622", "5b75539f8576e725", "5b7868dd6c3e9902", "5b9252acf017cdb0",
    "61058a8bd4669d8f", "660f5656e3e8f13c", "69d4211f8999a5de", "6db67ddbf0c9afdc", "70e1105aef854eac",
    "7358bd4ec877e1b6", "764c28b6f1a7bf8b", "784109ed21a6f7c6", "83dd9b6d31cdc845", "8417659f46b5b43b",
    "85fc59a791317965", "861827ebacf9b146", "8a50f733962ed5a9", "917ebb3396b2ff2e", "94baebcb5e5e0290",
    "96d47181cad1617b", "96e3955426b1ce0c", "985f077cd1bfec66", "9a046660abc6d16a", "9af5f7e4f3ea3794",
    "9c15e816069946fb", "9c4569379694d6a1", "9e48bad6e31c1282", "a4a04fefd9830943", "a744d93f2c6e8830",
    "a92f6bdb75789bcc", "a9f1fd8657329a37", "aa1a18a65f08ef46", "aa59343f9ebb354f", "ac85892205b92e8c",
    "b0fbddb4123cbb23", "b183731b00fc96f4", "b19ca7c8e2f1594f", "b1b0eba4b23c4f1f", "b2808c947ea04c19",
    "b451b7316b8a6749", "b51026e4444f98ec", "b5bced848634723f", "b5f2031eb62e37c6", "b8510932dad3ddf0",
    "b899a596c6e0b853", "b942714f0ecb8e42", "bac77a84e2628422", "bccae4ce7be65100", "bf15d6ce1d5e7f9c",
    "bf6861878156eaab", "c239b29d5de51f3b", "c40677cf313fa90d", "c5141636904a156a", "c5626fc850ed2e70",
    "c7935003b01d7134", "c798ceefaebca023", "c856d916814b6949", "c9cb65c5cd16af86", "ca75800d2eb00348",
    "cd2caff46d2cedb3", "d222a18d6474d081", "d254fa846ea7252e", "d3f35b83a4c7b3cf", "d5e34819841c4c11",
    "dd6c5828ddc6ba88", "e462aa653afd9459", "e5fd04f75adbf045", "e607ae52e4bc69c6", "e64cc85c8ee32b61",
    "e714b7d37de7c070", "e78673638c3bcdee", "e7992e13a75d613d", "eac42ad34c8f70c7", "ec80b5b888d60bef",
    "ef35ee2fbf0b1463", "ef7975a82504995e", "f081f7b8d4310e67", "f215c159a980ba59", "f2743cb33eb192cc",
    "f5af71a9b698cd30", "f5c1f28262e9cb53", "fba9db77bd480d9d", "fe1ab047c128a3c3",
}

NAME = {
    "1317dfa6a0c51245", "19e05df6b2e5fb94", "1a49bfba8f876f00", "1c467339c960a107", "2340486ee63d60f5",
    "25b1b6d01caeacde", "38102b4b8d453fba", "40a7e9acb06295d6", "43385991f0be5fed", "4e629e9dbcc153d7",
    "5911d0156c5d8e31", "5c509152b44d4cf0", "5d2c13d6f9fa4211", "6013de0381f648b7", "625fe74cad4600b5",
    "66ecdd9246ba554c", "7781113a99f17728", "7b3e7cdfdbca8bb5", "7e27f3f14e6b7c35", "80b1509ea3764325",
    "895d1ba9ca06739e", "970eaf0d4dc9bec3", "97a62ad21d79c01c", "9a13a9d1ca9254fb", "a448294a6283de8a",
    "a631f4488a457da2", "a84571394b5e99fe", "aeebad4a796fcc2e", "b4ef49fb50c0c07d", "b75097a30359c05f",
    "b769a6983b42d565", "ba47c5529523a474", "bea359bea8f7d88e", "c1d4a27a23459b51", "c34ce311989d8bf0",
    "c49fea7425fa7f86", "c51faa148557a08c", "c7575274a20d035b", "cbc17ed4ed1a1132", "d37d96b42ad43384",
    "da8575ae360c77ac", "dbc4a04327176e65", "dca6ec9510fc0176", "e636c973d999a634", "e7ac827c27b0acaa",
    "f405f95c2f4102f9", "fa5bf879acb91bd6", "fae0694da83ad615",
}


def h(word: str) -> str:
    """Truncated SHA-256 of a normalised (lower-case) word or phrase."""
    return hashlib.sha256(word.lower().encode()).hexdigest()[:16]


def candidates(text: str):
    """Every word, adjacent word pair, and hyphen-joined run ("ab-12" → "ab12") in `text`."""
    low = text.lower()
    words = re.findall(r"[a-z0-9]+", low)
    for w in words:
        yield w
    for a, b in zip(words, words[1:]):
        yield f"{a} {b}"
    for run in re.findall(r"[a-z0-9]+(?:-[a-z0-9]+)+", low):
        yield run.replace("-", "")


def hits(text: str, table: set[str]) -> list[str]:
    return sorted({c for c in candidates(text) if h(c) in table})


def main(argv: list[str]) -> int:
    if argv[:1] == ["--hash"]:
        print(h(" ".join(argv[1:])))
        return 0
    failures = []
    files = sorted({p for pat in SCAN for p in glob.glob(os.path.join(REPO, pat))})
    for path in files:
        rel = os.path.relpath(path, REPO)
        with open(path, encoding="utf-8") as f:
            text = f.read()
        for lineno, line in enumerate(text.splitlines(), 1):
            for word in hits(line, STRICT):
                failures.append(f"{rel}:{lineno}: banned name '{word}'")
            if line.lstrip().startswith("#") and rel.endswith(".md"):
                for word in hits(line, NAME):
                    failures.append(f"{rel}:{lineno}: nameplate word '{word}' in a heading")
        if rel.startswith("data/cars/") and rel.endswith(".json"):
            car = json.loads(text)
            for key in ("id", "display_name", "analog_of", "lattice_id"):
                for word in hits(str(car.get(key, "")).replace("_", " "), NAME):
                    failures.append(f"{rel}: nameplate word '{word}' in {key}")
    for line in failures:
        print(line)
    print(f"nameplate_check: scanned {len(files)} files, {len(failures)} problem(s)")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
