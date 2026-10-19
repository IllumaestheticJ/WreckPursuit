"""Sweep specs: parse a YAML (or JSON) sweep file and expand it into points.

A spec (full reference: tools/tuning-dash/README.md)::

    name: tire_lateral_b              # default: the file stem
    description: Street tire lateral B vs skidpad
    probe: {kind: vehicle_metrics, car: debug_sedan, metrics: [skidpad, step_steer]}
    params:
      - target: tire:{tire}:lateral.b # `{var}` comes from the variant's vars
        label: lateral B
        op: scale                     # set (default) | scale | add
        values: [0.8, 0.9, 1.0, 1.1, 1.2]   # or range: {from, to, steps} / {from, to, step}
    combine: grid                     # grid (default) | zip | each
    variants:                         # side-by-side comparison; crossed with the params
      - {name: sedan, vars: {tire: street}}
      - {name: coupe, vars: {tire: sport}, probe: {car: debug_coupe}}
    overrides: []                     # fixed overrides for every point
    expect: [{metric: skidpad_max_lat_g, min: 0.8, max: 1.0}]   # variants may add their own
    workers: 4

Each point is one probe job: the variant's probe, the fixed overrides, the variant's `set`
overrides, then one override per (param, target) for the point's values.
"""

from __future__ import annotations

import copy
import json
import math
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

OPS = ("set", "scale", "add")
COMBINES = ("grid", "zip", "each")
PROBE_KINDS = ("scenario", "vehicle_metrics", "barrier", "barrier_scenario")
_NAME_RE = re.compile(r"^[A-Za-z0-9_.-]+$")


class SpecError(ValueError):
    """A sweep spec is malformed."""


@dataclass
class Param:
    """One swept parameter: one or more targets moved together by the same value."""

    targets: list[str]
    label: str
    op: str
    values: list[Any]
    round_sig: int | None = None


@dataclass
class Variant:
    """One side of a comparison (a car, a mode, an impact kind...)."""

    name: str
    probe: dict[str, Any] = field(default_factory=dict)
    vars: dict[str, str] = field(default_factory=dict)
    set: list[dict[str, Any]] = field(default_factory=list)
    expect: list[dict[str, Any]] = field(default_factory=list)


@dataclass
class Point:
    """One probe run."""

    id: str
    variant: str
    params: dict[str, Any]
    job: dict[str, Any]
    is_base: bool = False
    expect: list[dict[str, Any]] = field(default_factory=list)


@dataclass
class Spec:
    """A parsed sweep spec."""

    name: str
    description: str
    probe: dict[str, Any]
    params: list[Param]
    combine: str
    variants: list[Variant]
    overrides: list[dict[str, Any]]
    expect: list[dict[str, Any]]
    workers: int
    plots: list[str]
    data: str | None
    raw: dict[str, Any]


def load_text(path: Path) -> dict[str, Any]:
    """Read a .yaml/.yml/.json spec into a dict."""
    text = path.read_text(encoding="utf-8")
    if path.suffix == ".json":
        doc = json.loads(text)
    else:
        try:
            import yaml  # PyYAML
        except ImportError as e:  # pragma: no cover - depends on the machine
            raise SpecError(
                f"{path}: reading YAML needs PyYAML (`pip install pyyaml`), "
                "or write the spec as .json"
            ) from e
        doc = yaml.safe_load(text)
    if not isinstance(doc, dict):
        raise SpecError(f"{path}: a spec is a mapping at the top level")
    return doc


def _num(v: Any, what: str) -> float:
    if isinstance(v, bool) or not isinstance(v, (int, float)):
        raise SpecError(f"{what} must be a number, got {v!r}")
    return float(v)


def expand_values(p: dict[str, Any], where: str) -> list[Any]:
    """`values: [...]` or `range: {from, to, steps|step}` → list of values."""
    if "values" in p and "range" in p:
        raise SpecError(f"{where}: give `values` or `range`, not both")
    if "values" in p:
        vals = p["values"]
        if not isinstance(vals, list) or not vals:
            raise SpecError(f"{where}: `values` must be a non-empty list")
        return list(vals)
    if "range" in p:
        r = p["range"]
        if not isinstance(r, dict):
            raise SpecError(f"{where}: `range` must be a mapping")
        lo = _num(r.get("from"), f"{where}: range.from")
        hi = _num(r.get("to"), f"{where}: range.to")
        if "steps" in r:
            n = int(_num(r["steps"], f"{where}: range.steps"))
            if n < 2:
                raise SpecError(f"{where}: range.steps must be >= 2")
            vals = [lo + (hi - lo) * i / (n - 1) for i in range(n)]
        elif "step" in r:
            st = _num(r["step"], f"{where}: range.step")
            if st <= 0 or (hi - lo) / st > 10000:
                raise SpecError(f"{where}: range.step must be > 0 and give <= 10000 values")
            n = int(math.floor((hi - lo) / st + 1e-9)) + 1
            vals = [lo + st * i for i in range(n)]
        else:
            raise SpecError(f"{where}: range needs `steps` or `step`")
        return [float(f"{v:.10g}") for v in vals]
    raise SpecError(f"{where}: needs `values` or `range`")


def parse(doc: dict[str, Any], default_name: str = "sweep") -> Spec:
    """Validate a spec dict."""
    known = {
        "name", "description", "probe", "params", "combine", "variants", "overrides",
        "expect", "workers", "plots", "data",
    }
    extra = set(doc) - known
    if extra:
        raise SpecError(f"unknown spec keys: {sorted(extra)}")
    name = str(doc.get("name", default_name))
    if not _NAME_RE.match(name):
        raise SpecError(f"name {name!r}: use letters, digits, '_', '-', '.'")
    probe = doc.get("probe")
    if not isinstance(probe, dict) or probe.get("kind") not in PROBE_KINDS:
        raise SpecError(f"probe.kind must be one of {PROBE_KINDS}")
    params = []
    for i, p in enumerate(doc.get("params") or []):
        where = f"params[{i}]"
        if not isinstance(p, dict):
            raise SpecError(f"{where}: must be a mapping")
        bad = set(p) - {"target", "targets", "label", "op", "values", "range", "round_sig"}
        if bad:
            raise SpecError(f"{where}: unknown keys {sorted(bad)}")
        targets = p.get("targets", p.get("target"))
        if isinstance(targets, str):
            targets = [targets]
        if not targets or not all(isinstance(t, str) and ":" in t for t in targets):
            raise SpecError(f"{where}: `target` (or `targets`) must be `<file>:<field path>`")
        op = p.get("op", "set")
        if op not in OPS:
            raise SpecError(f"{where}: op must be one of {OPS}")
        values = expand_values(p, where)
        if op != "set":
            for v in values:
                _num(v, f"{where}: {op} value")
        label = str(p.get("label") or targets[0].rsplit(":", 1)[-1])
        params.append(Param(targets, label, op, values, p.get("round_sig")))
    labels = [p.label for p in params]
    if len(set(labels)) != len(labels):
        raise SpecError(f"param labels must be unique: {labels}")
    combine = doc.get("combine", "grid")
    if combine not in COMBINES:
        raise SpecError(f"combine must be one of {COMBINES}")
    if combine == "zip" and len({len(p.values) for p in params}) > 1:
        raise SpecError("combine: zip needs every param to have the same number of values")
    variants = []
    for i, v in enumerate(doc.get("variants") or [{"name": "base"}]):
        if not isinstance(v, dict) or "name" not in v:
            raise SpecError(f"variants[{i}]: needs a name")
        bad = set(v) - {"name", "probe", "vars", "set", "expect"}
        if bad:
            raise SpecError(f"variants[{i}]: unknown keys {sorted(bad)}")
        variants.append(
            Variant(
                name=str(v["name"]),
                probe=dict(v.get("probe") or {}),
                vars={k: str(x) for k, x in (v.get("vars") or {}).items()},
                set=list(v.get("set") or []),
                expect=_expect_list(v.get("expect"), f"variants[{i}].expect"),
            )
        )
    if len({v.name for v in variants}) != len(variants):
        raise SpecError("variant names must be unique")
    expect = _expect_list(doc.get("expect"), "expect")
    workers = int(doc.get("workers", 0) or 0)
    return Spec(
        name=name,
        description=str(doc.get("description", "")),
        probe=probe,
        params=params,
        combine=combine,
        variants=variants,
        overrides=list(doc.get("overrides") or []),
        expect=expect,
        workers=workers,
        plots=[str(x) for x in doc.get("plots") or []],
        data=doc.get("data"),
        raw=doc,
    )


def _expect_list(raw: Any, where: str) -> list[dict[str, Any]]:
    out = list(raw or [])
    for i, e in enumerate(out):
        if not isinstance(e, dict) or "metric" not in e or not ({"min", "max"} & set(e)):
            raise SpecError(f"{where}[{i}]: needs `metric` and `min` and/or `max`")
    return out


def load(path: Path) -> Spec:
    """Load and validate a spec file."""
    return parse(load_text(path), default_name=path.stem)


def _combos(spec: Spec) -> list[tuple[dict[str, Any], bool]]:
    """(param label → value, is_base) for every parameter combination."""
    if not spec.params:
        return [({}, True)]
    if spec.combine == "zip":
        n = len(spec.params[0].values)
        return [({p.label: p.values[i] for p in spec.params}, False) for i in range(n)]
    if spec.combine == "each":
        out: list[tuple[dict[str, Any], bool]] = []
        for p in spec.params:
            for v in p.values:
                out.append(({p.label: v}, _neutral(p, v)))
        return out
    combos: list[dict[str, Any]] = [{}]
    for p in spec.params:
        combos = [dict(c, **{p.label: v}) for c in combos for v in p.values]
    return [(c, False) for c in combos]


def _neutral(p: Param, v: Any) -> bool:
    return (p.op == "scale" and v == 1) or (p.op == "add" and v == 0)


def _fill(template: str, vars: dict[str, str], where: str) -> str:
    try:
        return template.format_map(vars)
    except KeyError as e:
        raise SpecError(f"{where}: {template!r} uses {{{e.args[0]}}} but the variant has no such var")


def points(spec: Spec, data: str) -> list[Point]:
    """Expand a spec into probe jobs, in a fixed order (variant-major)."""
    out = []
    params = {p.label: p for p in spec.params}
    n = 0
    for var in spec.variants:
        probe = copy.deepcopy(spec.probe)
        probe.update(copy.deepcopy(var.probe))
        vars = {k: str(v) for k, v in probe.items() if isinstance(v, (str, int, float))}
        vars.update(var.vars)
        fixed = []
        for ov in list(spec.overrides) + list(var.set):
            ov = dict(ov)
            ov["target"] = _fill(str(ov.get("target", "")), vars, f"variant {var.name}")
            fixed.append(ov)
        for combo, is_base in _combos(spec):
            ovs = list(fixed)
            for label, value in combo.items():
                p = params[label]
                for t in p.targets:
                    ov: dict[str, Any] = {
                        "target": _fill(t, vars, f"param {label}"),
                        "op": p.op,
                        "value": value,
                    }
                    if p.round_sig is not None:
                        ov["round_sig"] = p.round_sig
                    ovs.append(ov)
            n += 1
            out.append(
                Point(
                    id=f"p{n:04d}",
                    variant=var.name,
                    params=combo,
                    job={"data": data, "overrides": ovs, "probe": probe},
                    is_base=is_base,
                    expect=list(spec.expect) + list(var.expect),
                )
            )
    return out
