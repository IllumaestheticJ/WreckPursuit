"""Tests for tools/tuning-dash. Run: `python3 -m unittest discover -s tools/tuning-dash/tests`.

The end-to-end test builds `tuning-probe` (cargo) and runs a real sweep + promote against a
temporary copy of `data/`, so nothing in the repo changes.
"""

import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from tuning_dash import actions, cli, runner  # noqa: E402
from tuning_dash import spec as specmod  # noqa: E402


def parse(doc):
    return specmod.parse(doc, "t")


class SpecTests(unittest.TestCase):
    def test_grid_zip_each(self):
        base = {
            "probe": {"kind": "vehicle_metrics", "car": "debug_sedan"},
            "params": [
                {"target": "tire:street:lateral.b", "label": "B", "op": "scale", "values": [0.9, 1.0, 1.1]},
                {"target": "tire:street:lateral.c", "label": "C", "op": "scale", "range": {"from": 0.8, "to": 1.2, "steps": 3}},
            ],
        }
        self.assertEqual(len(specmod.points(parse(base), "data")), 9)
        self.assertEqual(len(specmod.points(parse(dict(base, combine="zip")), "data")), 3)
        each = specmod.points(parse(dict(base, combine="each")), "data")
        self.assertEqual(len(each), 6)
        self.assertEqual([p.is_base for p in each], [False, True, False, False, True, False])
        self.assertEqual(each[0].job["overrides"], [{"target": "tire:street:lateral.b", "op": "scale", "value": 0.9}])

    def test_range_step_and_errors(self):
        self.assertEqual(specmod.expand_values({"range": {"from": 10, "to": 20, "step": 5}}, "x"), [10.0, 15.0, 20.0])
        bad = [
            {"probe": {"kind": "nope"}},
            {"probe": {"kind": "barrier"}, "params": [{"target": "nocolon", "values": [1]}]},
            {"probe": {"kind": "barrier"}, "params": [{"target": "a:b", "op": "scale", "values": ["x"]}]},
            {"probe": {"kind": "barrier"}, "params": [{"target": "a:b", "values": [1], "range": {}}]},
            {"probe": {"kind": "barrier"}, "combine": "zip", "params": [
                {"target": "a:b", "label": "x", "values": [1, 2]}, {"target": "a:c", "label": "y", "values": [1]}]},
            {"probe": {"kind": "barrier"}, "bogus": 1},
            {"probe": {"kind": "barrier"}, "expect": [{"metric": "x"}]},
            {"probe": {"kind": "barrier"}, "name": "has space"},
        ]
        for doc in bad:
            with self.assertRaises(specmod.SpecError, msg=str(doc)):
                parse(doc)

    def test_variants_vars_and_expect(self):
        sp = parse({
            "probe": {"kind": "vehicle_metrics"},
            "params": [{"target": "tire:{tire}:lateral.b", "values": [1.0]}],
            "variants": [
                {"name": "s", "vars": {"tire": "street"}, "probe": {"car": "debug_sedan"},
                 "expect": [{"metric": "m", "min": 1}]},
                {"name": "c", "vars": {"tire": "sport"}, "probe": {"car": "debug_coupe"}},
            ],
            "expect": [{"metric": "g", "max": 2}],
        })
        pts = specmod.points(sp, "data")
        self.assertEqual(pts[1].job["overrides"][0]["target"], "tire:sport:lateral.b")
        self.assertEqual(pts[1].job["probe"]["car"], "debug_coupe")
        self.assertEqual(len(pts[0].expect), 2)
        self.assertEqual(len(pts[1].expect), 1)
        with self.assertRaises(specmod.SpecError):
            specmod.points(parse({"probe": {"kind": "barrier"}, "params": [{"target": "car:{nope}:x", "values": [1]}]}), "data")

    def test_expect_evaluation(self):
        checks = runner.evaluate_expect(
            [{"metric": "a", "min": 1, "max": 2}, {"metric": "b", "min": 0}, {"metric": "c", "max": 1, "pending": True}],
            {"a": 1.5, "b": None, "c": 3},
        )
        self.assertEqual([c["passed"] for c in checks], [True, False, False])
        self.assertTrue(checks[2]["pending"])

    def test_every_preset_is_valid(self):
        presets = sorted(cli.PRESETS.glob("*.yaml"))
        self.assertGreaterEqual(len(presets), 8)
        for p in presets:
            sp = specmod.load(p)
            self.assertTrue(specmod.points(sp, "data"), p.name)


class EndToEnd(unittest.TestCase):
    """Real probe: sweep into a temp out folder over a temp data copy, then promote."""

    @classmethod
    def setUpClass(cls):
        cls.exe = runner.probe_binary(build=True)
        cls.tmp = Path(tempfile.mkdtemp(prefix="tuning-dash-test-"))
        cls.data = cls.tmp / "data"
        for sub in ("cars", "tires", "lattices", "materials", "modes", "police", "city"):
            shutil.copytree(runner.REPO / "data" / sub, cls.data / sub)
        cls.out = cls.tmp / "sweeps"

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.tmp, ignore_errors=True)

    def test_sweep_promote_stale(self):
        sp = parse({
            "name": "e2e",
            "data": str(self.data),
            "probe": {"kind": "barrier", "car": "debug_sedan", "mode": "Simulation", "impact": "Frontal", "speeds_kmh": [25, 56]},
            "params": [{"target": "mode:sim:impulse_to_lattice_scale", "values": [0.8, 1.0]}],
            "expect": [{"metric": "crush_m@56", "min": 0.0, "max": 5.0}],
        })
        d = runner.run(sp, build=False, out_root=self.out, workers=2)
        doc = json.loads((d / "sweep.json").read_text())
        self.assertEqual(len(doc["points"]), 2)
        p1, p2 = doc["points"]
        self.assertTrue(p1["ok"] and p2["ok"], (p1.get("error"), p2.get("error")))
        self.assertTrue(p1["passed"])
        self.assertEqual(p1["series"]["x"], [25.0, 56.0])
        # Less impulse into the lattice → less crush at 56 km/h. (plasticity_scale is not
        # monotonic in crush any more after round 6b's damage merge; see the integration doc.)
        self.assertLess(p1["metrics"]["crush_m@56"], p2["metrics"]["crush_m@56"])
        self.assertEqual(p1["applied"][0]["pointer"], "/impulse_to_lattice_scale")

        # Second run reuses both points.
        d = runner.run(sp, build=False, out_root=self.out)
        self.assertTrue(all(p["reused"] for p in json.loads((d / "sweep.json").read_text())["points"]))

        sim = self.data / "modes" / "sim.toml"
        before = sim.read_text()
        o = actions.promote_point("e2e", "p0002", root=self.out)
        self.assertFalse(o["ok"], "1.0 equals the file value: nothing to promote")
        o = actions.promote_point("e2e", "p0001", note="e2e test", root=self.out)
        self.assertTrue(o["ok"], o)
        after = sim.read_text()
        diff = [(a, b) for a, b in zip(before.splitlines(), after.splitlines()) if a != b]
        self.assertEqual(len(diff), 1)
        self.assertTrue(diff[0][1].startswith("impulse_to_lattice_scale = 0.8"))
        log = (self.data / "TUNING_CHANGELOG.md").read_text()
        self.assertIn("`modes/sim.toml`: `/impulse_to_lattice_scale` 1.0 → 0.8 · from sweep `e2e` point p0001", log)
        self.assertIn("e2e test", log)

        # The sweep now started from a value the file no longer holds: refused unless forced.
        sim.write_text(after.replace("impulse_to_lattice_scale = 0.8", "impulse_to_lattice_scale = 0.9"))
        o = actions.promote_point("e2e", "p0001", root=self.out)
        self.assertFalse(o["ok"])
        self.assertIn("stale", o["error"])

    def test_optional_field_created_and_promoted(self):
        sp = parse({
            "name": "e2e_opt",
            "data": str(self.data),
            "probe": {"kind": "vehicle_metrics", "car": "debug_sedan", "metrics": ["zero_to_100"]},
            "params": [{"target": "car:debug_sedan:drivetrain.efficiency", "values": [0.85, 0.95]}],
        })
        d = runner.run(sp, build=False, out_root=self.out)
        pts = json.loads((d / "sweep.json").read_text())["points"]
        self.assertTrue(all(p["ok"] for p in pts), [p.get("error") for p in pts])
        self.assertIsNone(pts[0]["applied"][0]["old"])
        # Higher efficiency → quicker 0-100.
        self.assertLess(pts[1]["metrics"]["zero_to_100_s"], pts[0]["metrics"]["zero_to_100_s"])
        car = self.data / "cars" / "debug_sedan.json"
        before = car.read_text()
        o = actions.promote_point("e2e_opt", pts[1]["id"], root=self.out)
        self.assertTrue(o["ok"], o)
        doc = json.loads(car.read_text())
        self.assertEqual(doc["drivetrain"]["efficiency"], 0.95)
        added = [l for l in car.read_text().splitlines() if l not in before.splitlines()]
        self.assertTrue(any('"efficiency": 0.95' in l for l in added), added)
        car.write_text(before)

    def test_scenario_probe_and_diff(self):
        sp = parse({
            "name": "e2e_scn",
            "data": str(self.data),
            "probe": {"kind": "scenario", "file": "apps/simcli/scenarios/vehicle/sedan_brake.ron"},
            "params": [{"target": "car:debug_sedan:brakes.max_torque_front_nm", "op": "scale", "values": [1.0]}],
        })
        d = runner.run(sp, build=False, out_root=self.out)
        p = json.loads((d / "sweep.json").read_text())["points"][0]
        self.assertTrue(p["ok"], p.get("error"))
        self.assertIn(p["passed"], (True, False))
        self.assertIn("car.distance_m", p["metrics"])
        self.assertTrue(p["thumb"]["car"])
        self.assertTrue((d / p["artifacts"][0]).exists())
        # Top-down frames: evenly spaced, the last one at the end of the run.
        self.assertTrue(p["frames"])
        self.assertTrue(all((d / f).exists() for f in p["frames"]))
        self.assertIn("end", p["info"]["frames"][-1])
        df = actions.diff_files("mode:arcade", "mode:sim", data=str(self.data))
        self.assertGreater(df["count"], 3)

    def test_survival_probe(self):
        sp = parse({
            "name": "e2e_surv",
            "data": str(self.data),
            "probe": {"kind": "survival", "car": "debug_sedan", "impact": "Frontal", "speeds_kmh": [56]},
            "variants": [{"name": "arc", "probe": {"mode": "Arcade"}}, {"name": "sim", "probe": {"mode": "Simulation"}}],
        })
        d = runner.run(sp, build=False, out_root=self.out)
        pts = {p["variant"]: p for p in json.loads((d / "sweep.json").read_text())["points"]}
        for p in pts.values():
            self.assertTrue(p["ok"], p.get("error"))
        # Simulation: one hard hit at 56 km/h is a total loss.
        self.assertEqual(pts["sim"]["metrics"]["hits_to_total_loss@56"], 1)
        self.assertGreater(pts["arc"]["metrics"]["health_after_1@56"], pts["sim"]["metrics"]["health_after_1@56"])

    def test_pursuit_probe_reads_police_overrides(self):
        sp = parse({
            "name": "e2e_pursuit",
            "data": str(self.data),
            "probe": {
                "kind": "pursuit",
                "file": "tools/tuning-dash/scenarios/racer_into_stopped_cop.ron",
                "police_entities": ["cop"],
            },
            "params": [{"target": "police:pursuit:patrol_speed_factor", "values": [0.9]}],
        })
        d = runner.run(sp, build=False, out_root=self.out)
        p = json.loads((d / "sweep.json").read_text())["points"][0]
        self.assertTrue(p["ok"], p.get("error"))
        self.assertIn("police/pursuit.toml", [a["file"] for a in p["applied"]])
        # The racer rear-ends the stand-in police car held stopped (~1.24 s).
        self.assertEqual(p["metrics"]["hits_moving_scenario"], 1)
        self.assertTrue(p["frames"])


if __name__ == "__main__":
    unittest.main()
