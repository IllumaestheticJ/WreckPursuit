"""Run a sweep: one `tuning-probe run` process per point, in parallel, into out/sweeps/<name>/.

Layout::

    out/sweeps/<name>/
      sweep.json                  spec, points, results, timing (what the dashboard reads)
      points/p0001/job.json       the probe job
      points/p0001/result.json    the probe result (+ simcli outputs for scenario probes)

A point whose job.json is unchanged and has a result.json is reused (pass `force` to re-run),
so editing one param of a spec and re-running only measures the new points.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict
from pathlib import Path
from typing import Any, Callable

from . import spec as specmod

REPO = Path(__file__).resolve().parents[3]
"""Repository root (tools/tuning-dash/tuning_dash/ → ../../..)."""

SWEEPS = REPO / "out" / "sweeps"
"""Where sweeps are written."""


def probe_binary(build: bool = True) -> Path:
    """Path to the release `tuning-probe` binary, building it first unless `build` is False.

    `TUNING_PROBE` in the environment overrides the path (and skips the build).
    """
    env = os.environ.get("TUNING_PROBE")
    if env:
        return Path(env)
    exe = REPO / "target" / "release" / ("tuning-probe.exe" if os.name == "nt" else "tuning-probe")
    if build:
        r = subprocess.run(
            ["cargo", "build", "--release", "-q", "-p", "tuning-probe"],
            cwd=REPO,
        )
        if r.returncode != 0:
            raise RuntimeError("cargo build -p tuning-probe failed")
    if not exe.exists():
        raise RuntimeError(f"{exe} not found; run `cargo build --release -p tuning-probe`")
    return exe


def evaluate_expect(expect: list[dict[str, Any]], metrics: dict[str, Any]) -> list[dict[str, Any]]:
    """Spec-level `expect` ranges → checks (a missing or null metric fails)."""
    checks = []
    for e in expect:
        m = e["metric"]
        v = metrics.get(m)
        lo, hi = e.get("min"), e.get("max")
        ok = v is not None and (lo is None or v >= lo) and (hi is None or v <= hi)
        rng = f"[{'-inf' if lo is None else lo}, {'inf' if hi is None else hi}]"
        checks.append(
            {
                "name": f"{m} in {rng}",
                "passed": ok,
                "actual": "missing" if v is None else f"{v:.4g}",
                "pending": bool(e.get("pending", False)),
            }
        )
    return checks


def _run_point(exe: Path, pdir: Path, job: dict[str, Any], force: bool) -> tuple[dict[str, Any], bool]:
    """Run (or reuse) one point. Returns (result, reused)."""
    pdir.mkdir(parents=True, exist_ok=True)
    job_path, res_path = pdir / "job.json", pdir / "result.json"
    text = json.dumps(job, indent=2, sort_keys=True) + "\n"
    if not force and res_path.exists() and job_path.exists() and job_path.read_text() == text:
        try:
            return json.loads(res_path.read_text()), True
        except json.JSONDecodeError:
            pass
    # Fresh run: clear old artifacts so nothing stale survives.
    for child in pdir.iterdir():
        if child.is_dir():
            shutil.rmtree(child)
        else:
            child.unlink()
    job_path.write_text(text)
    t0 = time.perf_counter()
    r = subprocess.run(
        [str(exe), "run", str(job_path), "--result", str(res_path)],
        cwd=REPO,
        capture_output=True,
        text=True,
    )
    wall = time.perf_counter() - t0
    if res_path.exists():
        result = json.loads(res_path.read_text())
    else:
        result = {"ok": False, "error": (r.stderr or r.stdout).strip() or f"exit {r.returncode}"}
    result["wall_s"] = round(wall, 3)
    res_path.write_text(json.dumps(result, indent=2) + "\n")
    return result, False


def run(
    spec: specmod.Spec,
    *,
    workers: int | None = None,
    force: bool = False,
    build: bool = True,
    out_root: Path = SWEEPS,
    spec_path: Path | None = None,
    progress: Callable[[int, int, str], None] | None = None,
) -> Path:
    """Run every point of `spec`; returns the sweep folder."""
    exe = probe_binary(build)
    data = spec.data or "data"
    pts = specmod.points(spec, data)
    sweep_dir = out_root / spec.name
    pdirs = sweep_dir / "points"
    pdirs.mkdir(parents=True, exist_ok=True)
    keep = {p.id for p in pts}
    for old in pdirs.iterdir():
        if old.is_dir() and old.name not in keep:
            shutil.rmtree(old)
    n_workers = workers or spec.workers or max(1, min(8, (os.cpu_count() or 2)))
    done = 0
    t0 = time.perf_counter()
    results: dict[str, tuple[dict[str, Any], bool]] = {}
    lock = threading.Lock()

    def job(p: specmod.Point) -> None:
        nonlocal done
        pj = dict(p.job)
        if pj["probe"].get("kind") == "scenario":
            pj["out"] = str(pdirs / p.id)  # simcli outputs land next to result.json
        res_reused = _run_point(exe, pdirs / p.id, pj, force)
        with lock:
            results[p.id] = res_reused
            done += 1
        if progress:
            res = results[p.id][0]
            progress(done, len(pts), f"{p.id} {p.variant} {p.params} {'ok' if res.get('ok') else 'ERROR ' + str(res.get('error'))}")

    with ThreadPoolExecutor(max_workers=n_workers) as ex:
        list(ex.map(job, pts))
    wall = time.perf_counter() - t0

    rows = []
    for p in pts:
        res, reused = results[p.id]
        checks = list(res.get("checks") or [])
        extra = evaluate_expect(p.expect, res.get("metrics") or {}) if res.get("ok") else []
        checks += extra
        hard = [c for c in checks if not c.get("pending")]
        if not res.get("ok"):
            passed = False
        elif hard:
            passed = all(c["passed"] for c in hard)
        else:
            passed = None
        rows.append(
            {
                "id": p.id,
                "variant": p.variant,
                "params": p.params,
                "is_base": p.is_base,
                "overrides": p.job["overrides"],
                "reused": reused,
                "ok": bool(res.get("ok")),
                "error": res.get("error"),
                "passed": passed,
                "checks": checks,
                "metrics": res.get("metrics") or {},
                "series": res.get("series"),
                "applied": res.get("applied") or [],
                "info": res.get("info") or {},
                "thumb": res.get("thumb") or {},
                "artifacts": [f"points/{p.id}/{a}" for a in res.get("artifacts") or []],
                "wall_s": res.get("wall_s"),
            }
        )
    doc = {
        "format": 1,
        "name": spec.name,
        "description": spec.description,
        "spec_path": _rel(spec_path),
        "spec": spec.raw,
        "probe_kind": spec.probe["kind"],
        "params": [asdict(p) for p in spec.params],
        "variants": [v.name for v in spec.variants],
        "combine": spec.combine,
        "plots": spec.plots,
        "data": data,
        "workers": n_workers,
        "wall_s": round(wall, 2),
        "finished_unix": int(time.time()),
        "points": rows,
    }
    (sweep_dir / "sweep.json").write_text(json.dumps(doc, indent=1) + "\n")
    return sweep_dir


def _rel(p: Path | None) -> str | None:
    if p is None:
        return None
    p = p.resolve()
    return str(p.relative_to(REPO)) if p.is_relative_to(REPO) else str(p)


def print_progress(done: int, total: int, msg: str) -> None:
    """Default CLI progress line."""
    print(f"[{done:>4}/{total}] {msg}", file=sys.stderr, flush=True)
