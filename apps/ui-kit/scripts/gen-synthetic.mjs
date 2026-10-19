// Writes fixtures/runs/synthetic_pursuit/{events.jsonl,metrics.csv,summary.json}.
//
// A hand-scripted 90 s night pursuit in the procedural city, in exactly the simcli output
// formats. It exists because the real sim emits no police events yet (machine E has not landed):
// the HUD heat meter, scanner feed and captain map need something to replay.
//
// - Car motion is a simple kinematic drive along grid roads (not physics).
// - Police events are `SimEvent::PoliceEvent { kind, payload }` with payload = UTF-8 JSON bytes of
//   the UI's proposed typed event (src/model/proposed.ts, request UI-1).
// - Region damage uses the v0.2.0-planned `RegionDamaged` / `GlassShattered` variants.
// - The economy event is a real `wp_economy::EconomyEvent` JSON payload.
//
//   node scripts/gen-synthetic.mjs
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const out = join(resolve(here, ".."), "fixtures/runs/synthetic_pursuit");
mkdirSync(out, { recursive: true });

const TPS = 240;
const DURATION_S = 90;
const SAMPLE_HZ = 10;
const PLAYER = 1;
const RIVAL = 2;

// Route along road centrelines (grid spacing 110 m, nodes at -605 + 110k).
const ROUTE_PLAYER = [
  [-495, -55], [275, -55], [275, 385], [-165, 385], [-165, 165], [495, 165], [495, -385], [-385, -385], [-385, 55],
];
const ROUTE_RIVAL = ROUTE_PLAYER.map(([x, y]) => [x, y]);

function drive(route, vMaxStraight, startDelayS) {
  // Kinematic follower: accelerate on straights, brake for corners.
  const samples = [];
  let seg = 0;
  let [x, y] = route[0];
  let v = 0;
  const accel = 6.5;
  const decel = 9;
  const vCorner = 13;
  for (let tick = 0; tick <= DURATION_S * TPS; tick++) {
    const t = tick / TPS;
    if (t >= startDelayS && seg < route.length - 1) {
      const [tx, ty] = route[seg + 1];
      const dx = tx - x;
      const dy = ty - y;
      const dist = Math.hypot(dx, dy);
      const brakeDist = (v * v - vCorner * vCorner) / (2 * decel);
      const target = dist < Math.max(brakeDist, 0) + 5 ? vCorner : vMaxStraight;
      v += Math.sign(target - v) * Math.min(Math.abs(target - v), (target > v ? accel : decel) / TPS);
      const step = v / TPS;
      if (step >= dist) {
        x = tx;
        y = ty;
        seg++;
      } else {
        x += (dx / dist) * step;
        y += (dy / dist) * step;
      }
    } else if (seg >= route.length - 1) {
      v = Math.max(0, v - decel / TPS);
    }
    samples.push({ t, tick, x, y, v });
  }
  return samples;
}

const player = drive(ROUTE_PLAYER, 42, 0.5);
const rival = drive(ROUTE_RIVAL, 39, 1.2);

function gearAndRpm(v) {
  // Six-speed box on a ~0.31 m wheel, final drive 3.9; shift at 6800 rpm.
  const ratios = [3.6, 2.2, 1.6, 1.25, 1.0, 0.82];
  for (let g = 0; g < ratios.length; g++) {
    const rpm = (v / 0.31) * ratios[g] * 3.9 * 9.549;
    if (rpm < 6800 || g === ratios.length - 1) return [g + 1, Math.max(900, Math.min(rpm, 7200))];
  }
  return [1, 900];
}

// ---- events ----
const events = [];
const at = (t, event) => events.push({ tick: Math.round(t * TPS), t_s: t, event });
const posAt = (samples, t) => {
  const s = samples[Math.min(samples.length - 1, Math.round(t * TPS))];
  return [Math.round(s.x * 10) / 10, Math.round(s.y * 10) / 10, 0];
};
const bytes = (obj) => [...Buffer.from(JSON.stringify(obj), "utf8")];
const KIND = { HeatChanged: 1, Spotted: 2, PursuitStarted: 3, LostVisual: 4, CooldownStarted: 5, Escaped: 6, Busted: 7, UnitDispatched: 8, UnitStatus: 9, ToolDeployed: 10, Radio: 11 };
const police = (t, payload) => {
  const kind = KIND[Object.keys(payload)[0]];
  at(t, { PoliceEvent: { kind, payload: bytes(payload) } });
};
const radio = (t, unit, text) => police(t, { Radio: { unit, text } });

at(3.0, { TrafficViolation: { vehicle: PLAYER, kind: { Speeding: { over_mps: 12.4 } } } });
at(3.4, { TrafficViolation: { vehicle: RIVAL, kind: { Speeding: { over_mps: 10.1 } } } });
police(6.0, { Spotted: { racer: PLAYER, unit: 10, pos: posAt(player, 6) } });
police(6.0, { HeatChanged: { racer: PLAYER, heat: 0.35, level: "L1" } });
radio(6.2, 10, "Adam-10, visual on a light coupe eastbound on 4th, speeding.");
police(8.0, { UnitDispatched: { unit: 11, kind: "Cruiser", target: posAt(player, 8) } });
police(8.1, { UnitDispatched: { unit: 12, kind: "Interceptor", target: posAt(player, 8) } });
radio(8.3, null, "All units, 10-80 in progress, two street racers heading north.");
police(10.0, { PursuitStarted: { racer: PLAYER, unit: 10 } });
police(10.0, { HeatChanged: { racer: PLAYER, heat: 0.6, level: "L1" } });
at(14.2, { TrafficViolation: { vehicle: PLAYER, kind: "RedLight" } });
at(15.0, { TrafficViolation: { vehicle: PLAYER, kind: "WrongWay" } });
police(18.0, { HeatChanged: { racer: PLAYER, heat: 0.2, level: "L2" } });
radio(18.2, 12, "Lincoln-12 joining, I have the lead car.");
// Clip a traffic car (entity 50).
const p22 = posAt(player, 22);
at(22.0, { Contact: { a: PLAYER, b: 50, point: [p22[0], p22[1] + 2.1, 0.5], normal: [0, 1, 0], impulse_ns: 6200, rel_speed: 9.5 } });
at(22.0, { TrafficViolation: { vehicle: PLAYER, kind: "Collision" } });
at(22.01, { RegionDamaged: { vehicle: PLAYER, region: "FrontBumper", severity: 0.42 } });
at(22.01, { RegionDamaged: { vehicle: PLAYER, region: "Hood", severity: 0.18 } });
at(22.02, { ComponentDamaged: { vehicle: PLAYER, component: "Lights", health: 0.55 } });
at(22.02, { ComponentDamaged: { vehicle: PLAYER, component: "Radiator", health: 0.81 } });
police(26.0, { ToolDeployed: { unit: 12, tool: "SpikeStrip", pos: posAt(player, 31) } });
radio(26.2, 12, "Spikes down at 9th and Harbor.");
police(28.0, { HeatChanged: { racer: PLAYER, heat: 0.55, level: "L2" } });
// Side swipe by unit 11.
const p30 = posAt(player, 30);
at(30.0, { Contact: { a: 11, b: PLAYER, point: [p30[0] - 0.9, p30[1], 0.6], normal: [1, 0, 0], impulse_ns: 9800, rel_speed: 7.1 } });
at(30.01, { RegionDamaged: { vehicle: PLAYER, region: "LeftDoorF", severity: 0.55 } });
at(30.01, { RegionDamaged: { vehicle: PLAYER, region: "FrontLeftFender", severity: 0.4 } });
at(30.02, { GlassShattered: { vehicle: PLAYER, region: "LeftDoorF" } });
at(30.03, { ComponentDamaged: { vehicle: PLAYER, component: "SuspensionFL", health: 0.62 } });
at(30.03, { ComponentDamaged: { vehicle: PLAYER, component: "SteeringRack", health: 0.78 } });
police(34.0, { HeatChanged: { racer: PLAYER, heat: 0.1, level: "L3" } });
radio(34.1, null, "Suspect rammed a unit. Authorised to use PIT. Air unit requested.");
police(40.0, { UnitDispatched: { unit: 13, kind: "Helicopter", target: posAt(player, 40) } });
police(42.0, { HeatChanged: { racer: PLAYER, heat: 0.45, level: "L3" } });
police(45.0, { LostVisual: { racer: PLAYER, unit: 10 } });
police(47.0, { CooldownStarted: { racer: PLAYER, seconds: 20 } });
radio(47.2, 10, "Lost him behind the warehouses, last seen southbound.");
police(52.0, { Spotted: { racer: PLAYER, unit: 13, pos: posAt(player, 52) } });
police(52.0, { PursuitStarted: { racer: PLAYER, unit: 13 } });
radio(52.1, 13, "Air-13, I have him. Southbound on Ring Road, lighting him up.");
police(55.0, { ToolDeployed: { unit: 13, tool: "Spotlight", pos: posAt(player, 55) } });
// Hard hit on a barrier.
const p60 = posAt(player, 60);
at(60.0, { Contact: { a: PLAYER, b: 4294967295, point: [p60[0], p60[1], 0.5], normal: [0, -1, 0], impulse_ns: 15200, rel_speed: 13.2 } });
at(60.01, { RegionDamaged: { vehicle: PLAYER, region: "FrontBumper", severity: 0.85 } });
at(60.01, { RegionDamaged: { vehicle: PLAYER, region: "FrameFront", severity: 0.35 } });
at(60.01, { RegionDamaged: { vehicle: PLAYER, region: "Hood", severity: 0.5 } });
at(60.02, { ComponentDamaged: { vehicle: PLAYER, component: "Radiator", health: 0.35 } });
at(60.02, { ComponentDamaged: { vehicle: PLAYER, component: "Engine", health: 0.72 } });
at(60.03, { ComponentFailed: { vehicle: PLAYER, component: "Lights" } });
at(60.2, { PartDetached: { vehicle: PLAYER, group: 1 } });
police(61.0, { HeatChanged: { racer: PLAYER, heat: 0.8, level: "L3" } });
police(68.0, { HeatChanged: { racer: PLAYER, heat: 0.15, level: "L4" } });
radio(68.1, null, "All units, suspect is now level four. Roadblocks authorised.");
police(69.0, { ToolDeployed: { unit: 11, tool: "Roadblock", pos: posAt(player, 80) } });
police(72.0, { LostVisual: { racer: PLAYER, unit: 13 } });
police(72.5, { CooldownStarted: { racer: PLAYER, seconds: 15 } });
radio(73.0, 13, "Lost visual in the tunnel. Searching.");
at(76.0, { ComponentDamaged: { vehicle: PLAYER, component: "Engine", health: 0.66 } });
police(87.5, { Escaped: { racer: PLAYER } });
police(87.5, { HeatChanged: { racer: PLAYER, heat: 0.0, level: "None" } });
radio(87.6, null, "All units, suspect evaded. Resume patrol.");
const econ = { player: 1, seq: 42, day: 7, reason: "BountyGained", car: 1, cash_delta: 0, rep_delta: 120, bounty_delta: 2500, cop_xp_delta: 0, cash_after: 12850 };
at(88.0, { EconomyEvent: { kind: 34, payload: bytes(econ) } });
events.sort((a, b) => a.tick - b.tick);

// Player health follows the hits.
const healthAt = (t) => (t < 22 ? 1 : t < 30 ? 0.93 : t < 60 ? 0.84 : t < 76 ? 0.66 : 0.64);

// ---- metrics ----
let csv = "t_s,tick,entity,x,y,z,speed_mps,lat_g,long_g,yaw_rate_rps,engine_rpm,gear,health,crush_depth_m\n";
const every = TPS / SAMPLE_HZ;
for (let tick = every; tick <= DURATION_S * TPS; tick += every) {
  for (const [name, samples, health] of [["racer", player, healthAt], ["rival", rival, () => 0.97]]) {
    const s = samples[tick];
    const prev = samples[tick - every];
    const [gear, rpm] = gearAndRpm(s.v);
    const longG = (s.v - prev.v) * SAMPLE_HZ / 9.81;
    csv += [
      (tick / TPS).toFixed(4), tick, name, s.x.toFixed(4), s.y.toFixed(4), "0.0000", s.v.toFixed(4),
      "0.0000", longG.toFixed(4), "0.0000", rpm.toFixed(1), gear, health(tick / TPS).toFixed(4),
      (name === "racer" && tick / TPS > 60 ? 0.21 : 0).toFixed(4),
    ].join(",") + "\n";
  }
}

const summary = {
  name: "synthetic_pursuit",
  synthetic: true,
  note: "Hand-scripted by apps/ui-kit/scripts/gen-synthetic.mjs; not simulation output.",
  contract_version: "0.1.0",
  ticks: DURATION_S * TPS,
  entities: [
    { id: "racer", entity: PLAYER, car: "shiro_turbo" },
    { id: "rival", entity: RIVAL, car: "kestrel_ts" },
  ],
};

writeFileSync(join(out, "events.jsonl"), events.map((e) => JSON.stringify(e)).join("\n") + "\n");
writeFileSync(join(out, "metrics.csv"), csv);
writeFileSync(join(out, "summary.json"), JSON.stringify(summary, null, 2) + "\n");
console.log(`fixtures/runs/synthetic_pursuit: ${events.length} events, ${csv.split("\n").length - 2} metric rows`);
