"""Promote and diff: thin wrappers over `tuning-probe promote` / `tuning-probe diff`."""

from __future__ import annotations

import datetime
import json
import subprocess
from pathlib import Path
from typing import Any

from .runner import REPO, SWEEPS, probe_binary


def load_sweep(name: str, root: Path = SWEEPS) -> dict[str, Any]:
    """Read out/sweeps/<name>/sweep.json."""
    p = root / name / "sweep.json"
    if not p.exists():
        raise FileNotFoundError(f"no sweep {name!r} ({p})")
    return json.loads(p.read_text())


def find_point(sweep: dict[str, Any], point_id: str) -> dict[str, Any]:
    """A point of a sweep by id."""
    for p in sweep["points"]:
        if p["id"] == point_id:
            return p
    raise KeyError(f"sweep {sweep['name']!r} has no point {point_id!r}")


def describe_params(params: dict[str, Any]) -> str:
    """`lateral B=0.9, springs=1.1`."""
    return ", ".join(f"{k}={v}" for k, v in params.items())


def promote_point(
    sweep_name: str,
    point_id: str,
    *,
    note: str = "",
    files: list[str] | None = None,
    dry_run: bool = False,
    force: bool = False,
    data: str | None = None,
    build: bool = False,
    root: Path = SWEEPS,
) -> dict[str, Any]:
    """Write a point's applied values into the data files (+ changelog line).

    `files` limits the promote to some of the point's files. Scenario overrides are never
    promoted. Returns the probe's outcome (`ok`, `error`, `files`, `log_lines`).
    """
    sweep = load_sweep(sweep_name, root)
    point = find_point(sweep, point_id)
    if not point.get("ok"):
        return {"ok": False, "error": f"{point_id} did not run: {point.get('error')}"}
    changes = [
        {"file": a["file"], "pointer": a["pointer"], "old": a["old"], "new": a["new"]}
        for a in point.get("applied", [])
        if a["file"] != "scenario" and (not files or a["file"] in files)
    ]
    if not changes:
        return {"ok": False, "error": "this point changes no data file (base point or scenario-only overrides)"}
    # When several overrides touched the same field, the last one is what was simulated.
    last: dict[tuple[str, str], dict[str, Any]] = {}
    first_old: dict[tuple[str, str], Any] = {}
    for c in changes:
        k = (c["file"], c["pointer"])
        first_old.setdefault(k, c["old"])
        last[k] = dict(c, old=first_old[k])
    source = f"sweep `{sweep_name}` point {point_id}"
    if point.get("variant") and len(sweep.get("variants", [])) > 1:
        source += f" [{point['variant']}]"
    if point.get("params"):
        source += f" ({describe_params(point['params'])})"
    req = {
        "changes": list(last.values()),
        "note": note,
        "source": source,
        "date": datetime.date.today().isoformat(),
        "force": force,
        "dry_run": dry_run,
    }
    return _probe_json(["promote", "-"], json.dumps(req), data or sweep.get("data") or "data", build)


def diff_files(a: str, b: str, *, data: str = "data", limit: int = 2000, build: bool = False) -> dict[str, Any]:
    """Field diff of two data files (data-root relative or shorthands like `car:x`)."""
    return _probe_json(["diff", a, b, "--limit", str(limit)], None, data, build)


def resolve_targets(targets: list[str], *, data: str = "data", build: bool = False) -> Any:
    """Current values every target selects."""
    return _probe_json(["resolve", *targets], None, data, build)


def _probe_json(args: list[str], stdin: str | None, data: str, build: bool) -> Any:
    exe = probe_binary(build)
    r = subprocess.run(
        [str(exe), *args, "--data", data],
        cwd=REPO,
        input=stdin,
        capture_output=True,
        text=True,
    )
    if r.stdout.strip():
        try:
            return json.loads(r.stdout)
        except json.JSONDecodeError:
            pass
    return {"ok": False, "error": (r.stderr or r.stdout).strip() or f"exit {r.returncode}"}


def data_files(data: str = "data") -> list[str]:
    """Data files a diff can pick from (cars, tires, modes, materials, lattices)."""
    root = REPO / data
    out = []
    for sub in ("cars", "tires", "modes", "materials", "lattices"):
        d = root / sub
        if d.is_dir():
            out += sorted(f"{sub}/{p.name}" for p in d.iterdir() if p.suffix in (".json", ".toml"))
    return out
