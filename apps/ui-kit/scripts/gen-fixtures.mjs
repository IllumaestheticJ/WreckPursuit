// Regenerates the mock-provider fixtures from real simulation output and real data.
//
//   npm run gen:fixtures
//
// 1. Runs `simcli` (release) on a few golden scenarios and copies `events.jsonl` +
//    `metrics.csv` into fixtures/runs/<scenario>/.
// 2. Derives a compact 2D map (one centre polyline per road, zones, POIs) from
//    data/city/<id>.road.json into fixtures/city/<id>.map.json for the captain / crew maps.
//
// The synthetic pursuit script (fixtures/runs/synthetic_pursuit/) is hand-written and is NOT
// touched by this script.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const pkg = resolve(here, "..");
const repo = resolve(pkg, "../..");

const SCENARIOS = ["golden/frontal_56kmh", "golden/tbone"];
const CITY = "procedural_12x12";

// ---- 1. simcli runs ----
const out = mkdtempSync(join(tmpdir(), "ui-kit-sim-"));
execFileSync("cargo", ["build", "--release", "--quiet", "-p", "simcli"], { cwd: repo, stdio: "inherit" });
for (const s of SCENARIOS) {
  execFileSync(join(repo, "target/release/simcli"), [
    "run", join(repo, "apps/simcli/scenarios", `${s}.ron`), "--out", out,
  ], { cwd: repo, stdio: "inherit" });
  const name = s.replace("/", "_");
  const dst = join(pkg, "fixtures/runs", name);
  mkdirSync(dst, { recursive: true });
  for (const f of ["events.jsonl", "metrics.csv", "summary.json"]) copyFileSync(join(out, name, f), join(dst, f));
  console.log(`fixtures/runs/${name}`);
}

// ---- 2. city map ----
const road = JSON.parse(readFileSync(join(repo, "data/city", `${CITY}.road.json`), "utf8"));
const round = (v) => Math.round(v * 10) / 10;
// Keep one lane per road (the first one seen) as the road's polyline.
const roads = new Map();
for (const lane of road.lanes) {
  if (lane.kind !== "Through" || roads.has(lane.road)) continue;
  roads.set(lane.road, {
    id: lane.road,
    speed_limit_mps: round(lane.speed_limit_mps),
    points: lane.polyline.map(([x, y]) => [round(x), round(y)]),
  });
}
const map = {
  id: road.id,
  source: `derived from data/city/${CITY}.road.json by scripts/gen-fixtures.mjs`,
  bounds_min: road.bounds_min,
  bounds_max: road.bounds_max,
  roads: [...roads.values()].sort((a, b) => a.id - b.id),
  intersections: road.nodes.map((n) => [round(n.pos[0]), round(n.pos[1])]),
  zones: road.zones.map((z, i) => ({ id: i, kind: z.kind, polygon: z.polygon.map(([x, y]) => [round(x), round(y)]) })),
  pois: road.pois.map((p) => ({ kind: p.kind, name: p.name, pos: [round(p.pos[0]), round(p.pos[1])] })),
};
mkdirSync(join(pkg, "fixtures/city"), { recursive: true });
writeFileSync(join(pkg, "fixtures/city", `${CITY}.map.json`), JSON.stringify(map));
console.log(`fixtures/city/${CITY}.map.json: ${map.roads.length} roads, ${map.zones.length} zones, ${map.pois.length} pois`);
