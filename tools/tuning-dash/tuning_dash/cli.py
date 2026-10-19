"""Command line: sweep, list, show, diff, promote, serve, presets."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from . import actions, runner
from . import spec as specmod

PRESETS = Path(__file__).resolve().parents[1] / "presets"
"""Standard sweeps (tools/tuning-dash/presets/*.yaml)."""


def resolve_spec_path(arg: str) -> Path:
    """A spec path, or a preset name (`tire_lateral_bcde`)."""
    p = Path(arg)
    if p.exists():
        return p
    for ext in (".yaml", ".yml", ".json"):
        q = PRESETS / f"{arg}{ext}"
        if q.exists():
            return q
    raise SystemExit(f"no spec file or preset named {arg!r} (see `tune.py presets`)")


def _fmt(v) -> str:
    if v is None:
        return "—"
    if isinstance(v, float):
        return f"{v:.4g}"
    return str(v)


def cmd_sweep(a: argparse.Namespace) -> int:
    path = resolve_spec_path(a.spec)
    sp = specmod.load(path)
    if a.name:
        sp.name = a.name
    pts = specmod.points(sp, sp.data or "data")
    if a.dry_run:
        for p in pts:
            print(f"{p.id} {p.variant:12} {actions.describe_params(p.params)}")
            for ov in p.job["overrides"]:
                print(f"       {ov.get('op', 'set'):5} {ov['target']} {ov['value']}")
        print(f"{len(pts)} points", file=sys.stderr)
        return 0
    d = runner.run(
        sp,
        workers=a.workers,
        force=a.force,
        build=not a.no_build,
        spec_path=path,
        progress=None if a.quiet else runner.print_progress,
    )
    doc = json.loads((d / "sweep.json").read_text())
    errors = [p for p in doc["points"] if not p["ok"]]
    failed = [p for p in doc["points"] if p["passed"] is False and p["ok"]]
    print(
        f"{doc['name']}: {len(doc['points'])} points in {doc['wall_s']} s "
        f"({sum(p['reused'] for p in doc['points'])} reused), "
        f"{len(failed)} failing checks, {len(errors)} errors → {d.relative_to(runner.REPO)}/sweep.json"
    )
    for p in errors:
        print(f"  ERROR {p['id']}: {p['error']}", file=sys.stderr)
    return 1 if errors else 0


def cmd_list(_: argparse.Namespace) -> int:
    if not runner.SWEEPS.is_dir():
        print("no sweeps yet (out/sweeps/ is empty)")
        return 0
    for d in sorted(runner.SWEEPS.iterdir()):
        f = d / "sweep.json"
        if f.exists():
            doc = json.loads(f.read_text())
            print(f"{doc['name']:28} {doc['probe_kind']:16} {len(doc['points']):4} points  {doc.get('description', '')}")
    return 0


def cmd_show(a: argparse.Namespace) -> int:
    doc = actions.load_sweep(a.sweep)
    metrics = a.metrics.split(",") if a.metrics else None
    for p in doc["points"]:
        if not p["ok"]:
            print(f"{p['id']} {p['variant']:10} ERROR {p['error']}")
            continue
        names = metrics or sorted(p["metrics"])[:8]
        vals = "  ".join(f"{m}={_fmt(p['metrics'].get(m))}" for m in names)
        status = {True: "pass", False: "FAIL", None: "    "}[p["passed"]]
        print(f"{p['id']} {p['variant']:10} {status} {actions.describe_params(p['params']):30} {vals}")
    return 0


def cmd_diff(a: argparse.Namespace) -> int:
    d = actions.diff_files(a.a, a.b, data=a.data, limit=a.limit, build=not a.no_build)
    if "rows" not in d:
        print(d.get("error"), file=sys.stderr)
        return 2
    print(f"{d['a']}  vs  {d['b']}: {d['count']} of {d['compared']} fields differ")
    for r in d["rows"]:
        rel = f"  ({r['rel'] * 100:+.1f} %)" if r.get("rel") is not None else ""
        print(f"  {r['pointer']:48} {_fmt(r['a']):>14} → {_fmt(r['b']):<14}{rel}")
    return 0


def cmd_promote(a: argparse.Namespace) -> int:
    o = actions.promote_point(
        a.sweep,
        a.point,
        note=a.note,
        files=a.file or None,
        dry_run=a.dry_run,
        force=a.force,
        build=not a.no_build,
    )
    if not o.get("ok"):
        print(f"refused: {o.get('error')}", file=sys.stderr)
        return 1
    verb = "would write" if a.dry_run else "wrote"
    print(f"{verb} {', '.join(o['files'])}")
    for line in o["log_lines"]:
        print(f"  {line}")
    return 0


def cmd_serve(a: argparse.Namespace) -> int:
    from . import server

    server.serve(a.host, a.port, build=not a.no_build)
    return 0


def cmd_presets(_: argparse.Namespace) -> int:
    for p in sorted(PRESETS.glob("*.yaml")):
        try:
            sp = specmod.load(p)
            n = len(specmod.points(sp, "data"))
            print(f"{p.stem:28} {sp.probe['kind']:16} {n:4} points  {sp.description}")
        except specmod.SpecError as e:
            print(f"{p.stem:28} INVALID: {e}")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="tune.py", description=__doc__)
    sub = ap.add_subparsers(dest="cmd", required=True)

    s = sub.add_parser("sweep", help="run a sweep spec (file or preset name)")
    s.add_argument("spec")
    s.add_argument("--workers", "-j", type=int)
    s.add_argument("--force", action="store_true", help="re-run points even if cached")
    s.add_argument("--name", help="override the sweep name (output folder)")
    s.add_argument("--dry-run", action="store_true", help="print the points, run nothing")
    s.add_argument("--no-build", action="store_true", help="do not cargo build the probe first")
    s.add_argument("--quiet", "-q", action="store_true")
    s.set_defaults(fn=cmd_sweep)

    s = sub.add_parser("list", help="list finished sweeps")
    s.set_defaults(fn=cmd_list)

    s = sub.add_parser("show", help="print a sweep's points as text")
    s.add_argument("sweep")
    s.add_argument("--metrics", help="comma-separated metric names")
    s.set_defaults(fn=cmd_show)

    s = sub.add_parser("diff", help="field diff of two data files (e.g. car:debug_sedan car:debug_coupe)")
    s.add_argument("a")
    s.add_argument("b")
    s.add_argument("--data", default="data")
    s.add_argument("--limit", type=int, default=200)
    s.add_argument("--no-build", action="store_true")
    s.set_defaults(fn=cmd_diff)

    s = sub.add_parser("promote", help="write a point's values into the data files + changelog")
    s.add_argument("sweep")
    s.add_argument("point", help="point id, e.g. p0007")
    s.add_argument("--note", default="", help="why (goes into the changelog line)")
    s.add_argument("--file", action="append", help="only promote this data file (repeatable)")
    s.add_argument("--dry-run", action="store_true")
    s.add_argument("--force", action="store_true", help="write even if the file changed since the sweep")
    s.add_argument("--no-build", action="store_true")
    s.set_defaults(fn=cmd_promote)

    s = sub.add_parser("serve", help="local dashboard (default http://127.0.0.1:8765)")
    s.add_argument("--host", default="127.0.0.1")
    s.add_argument("--port", type=int, default=8765)
    s.add_argument("--no-build", action="store_true")
    s.set_defaults(fn=cmd_serve)

    s = sub.add_parser("presets", help="list the standard sweeps")
    s.set_defaults(fn=cmd_presets)

    a = ap.parse_args(argv)
    try:
        return a.fn(a)
    except (specmod.SpecError, FileNotFoundError, KeyError, RuntimeError) as e:
        print(f"tune.py: {e}", file=sys.stderr)
        return 2
