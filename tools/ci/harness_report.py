#!/usr/bin/env python3
"""Harness report: what `cargo test --workspace` does *not* run, as Markdown.

CI appends it to the job summary so parked work stays visible:
1. every `#[ignore]`d Rust test with its reason (from `#[ignore = "..."]`; a bare `#[ignore]`
   shows "(no reason given)" plus the first line of the test's doc comment, if any);
2. every simcli scenario that debug `cargo test` skips (`scenarios/soak/` or `slow: true`),
   which only the release CI jobs (`simcli determinism`, `simcli goldens`) run.
Only the standard library is used.

Usage: tools/ci/harness_report.py [repo_root] [--strict]
  --strict   exit 1 if any ignored test has no reason (not used by CI yet).
"""

import os
import re
import sys

SKIP_DIRS = {"target", "node_modules", ".git", "dist", "pkg"}
IGNORE_RE = re.compile(r"#\[ignore\s*(?:=\s*\"((?:[^\"\\]|\\.)*)\")?\s*\]", re.S)
FN_RE = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)")


def clean_reason(raw):
    """Join a Rust string literal's `\\`-newline continuations and collapse whitespace."""
    text = re.sub(r"\\\n\s*", "", raw)
    text = text.replace('\\"', '"')
    return " ".join(text.split())


def doc_hint(src, start, end):
    """First `///` doc line next to the attribute (just above it, or between it and the fn)."""
    before = src[:start].rstrip().split("\n")
    lines = src[start:end].split("\n")
    docs = []
    i = len(before) - 1
    while i >= 0 and before[i].strip().startswith(("///", "#[")):
        docs.insert(0, before[i].strip())
        i -= 1
    docs += [ln.strip() for ln in lines]
    for ln in docs:
        if ln.startswith("///"):
            return ln[3:].strip()
    return None


def owner_of(path):
    """Crate or app folder a file lives in, e.g. `crates/wp_race`."""
    parts = path.split("/")
    return "/".join(parts[:2]) if len(parts) > 2 else parts[0]


def find_ignored(root):
    """Yield (path, line, test_name, reason) for every ignored test under `root`, sorted."""
    found = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS)
        for name in sorted(filenames):
            if not name.endswith(".rs"):
                continue
            full = os.path.join(dirpath, name)
            with open(full, encoding="utf-8") as f:
                src = f.read()
            for m in IGNORE_RE.finditer(src):
                # Skip mentions inside line comments (e.g. "Long runs are `#[ignore]`d").
                line_start = src.rfind("\n", 0, m.start()) + 1
                if "//" in src[line_start : m.start()]:
                    continue
                fn = FN_RE.search(src, m.end())
                test = fn.group(1) if fn else "?"
                reason = clean_reason(m.group(1)) if m.group(1) is not None else None
                doc = doc_hint(src, m.start(), fn.end() if fn else m.end())
                line = src.count("\n", 0, m.start()) + 1
                rel = os.path.relpath(full, root).replace(os.sep, "/")
                found.append((rel, line, test, reason, doc))
    return found


SLOW_RE = re.compile(r"^\s*slow\s*:\s*true\s*,", re.M)
NAME_RE = re.compile(r"^\s*name\s*:\s*\"([^\"]+)\"", re.M)


def release_only_scenarios(root):
    """(path, name, why) for scenarios debug `cargo test` skips (soak folder or `slow: true`)."""
    base = os.path.join(root, "apps", "simcli", "scenarios")
    out = []
    for dirpath, dirnames, filenames in os.walk(base):
        dirnames.sort()
        for name in sorted(filenames):
            if not name.endswith(".ron"):
                continue
            full = os.path.join(dirpath, name)
            with open(full, encoding="utf-8") as f:
                src = f.read()
            rel = os.path.relpath(full, root).replace(os.sep, "/")
            soak = "/soak/" in "/" + os.path.relpath(full, base).replace(os.sep, "/")
            if soak or SLOW_RE.search(src):
                m = NAME_RE.search(src)
                out.append((rel, m.group(1) if m else name, "soak/" if soak else "slow: true"))
    return out


def print_release_only_scenarios(root):
    """Markdown table of `release_only_scenarios`."""
    rows = release_only_scenarios(root)
    print(f"### Release-only scenarios ({len(rows)})\n")
    print("Debug `cargo test` skips these; CI's release jobs run them (determinism, goldens).\n")
    print("| Scenario | Why | File |")
    print("|---|---|---|")
    for path, name, why in rows:
        print(f"| `{name}` | {why} | `{path}` |")
    print()


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    strict = "--strict" in sys.argv[1:]
    root = args[0] if args else os.path.join(os.path.dirname(__file__), "..", "..")
    rows = find_ignored(os.path.abspath(root))
    missing = sum(1 for r in rows if r[3] is None)
    print("## Harness report\n")
    print(f"### Ignored tests ({len(rows)}, {missing} without a reason)\n")
    print("Run one with `cargo test -p <crate> --release -- --ignored <name>`.\n")
    print("| Crate | Test | Reason | Where |")
    print("|---|---|---|---|")
    for path, line, test, reason, doc in rows:
        why = reason if reason else "**(no reason given)**" + (f" doc: {doc}" if doc else "")
        why = why.replace("|", "\\|")
        print(f"| `{owner_of(path)}` | `{test}` | {why} | `{path}:{line}` |")
    print()
    print_release_only_scenarios(os.path.abspath(root))
    return 1 if strict and missing else 0


if __name__ == "__main__":
    sys.exit(main())
