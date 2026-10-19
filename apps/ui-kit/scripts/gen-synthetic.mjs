// Writes fixtures/runs/synthetic_pursuit/{events.jsonl,metrics.csv,summary.json}.
//
// A hand-scripted 90 s night race + pursuit in the data/city map, in exactly the simcli output
// formats. It covers what no real run has yet: damage *and* police *and* a race in one stream
// (`police_pursuit` is the recorded wp_police run; the simcli runs are crashes only).
//
// - Car motion is a simple kinematic drive along grid roads (not physics).
// - Police events are real `wp_police::PoliceEvent` shapes (src/model/police.ts) in
//   `SimEvent::PoliceEvent { kind, payload }`, payload = UTF-8 JSON, kind = wp_police::events::kind.
// - Damage uses the contract v0.2.0 `RegionDamaged` / `GlassShattered` variants;
//   `TrafficViolation` carries `pos`.
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
// wp_police::events::kind
const KIND = { Radio: 1, WantedLevelChanged: 2, PursuitStarted: 3, PursuitEnded: 4, UnitSpawned: 5, UnitDespawned: 6, UnitDisabled: 7, RoadblockPlaced: 8, SpikesPlaced: 9, ToolRemoved: 10, SpikeHit: 11, HeloLaunched: 12, PitAttempt: 13 };
const police = (t, payload) => at(t, { PoliceEvent: { kind: KIND[Object.keys(payload)[0]], payload: bytes(payload) } });
const radio = (t, unit, text) => police(t, { Radio: { unit, text } });
const level = (t, l) => police(t, { WantedLevelChanged: { racer: PLAYER, level: l } });
const spawn = (t, unit, kind, p) => police(t, { UnitSpawned: { unit, kind, pos: p, heading: [0, 1] } });
const violation = (t, vehicle, kind, samples) => at(t, { TrafficViolation: { vehicle, kind, pos: posAt(samples, t) } });
const off = (p, dx, dy) => [p[0] + dx, p[1] + dy, 0];

spawn(0.1, 1, "Car", [-220, 340, 0]);
spawn(0.1, 2, "Car", [-180, 300, 0]);
spawn(0.1, 3, "Car", [380, -60, 0]);
radio(0.1, 1, "Unit 1 resuming patrol, Core.");
radio(0.1, 3, "Unit 3 resuming patrol, Industrial.");
violation(3.0, PLAYER, { Speeding: { over_mps: 12.4 } }, player);
violation(3.4, RIVAL, { Speeding: { over_mps: 10.1 } }, rival);
level(4.0, "L1");
radio(4.2, null, "Dispatch: caller reports two street racers, Core.");
level(9.0, "L2");
police(10.0, { PursuitStarted: { racer: PLAYER, unit: 3 } });
radio(10.0, 3, "Unit 3 in pursuit of racer #1 northbound in Core.");
radio(10.1, null, "Dispatch: copy, sending unit 1 of 3 on racer #1.");
violation(14.2, PLAYER, "RedLight", player);
violation(15.0, PLAYER, "WrongWay", player);
spawn(18.0, 4, "Car", off(posAt(player, 18), 400, 200));
radio(18.1, 4, "Unit 4 moving to intercept racer #1 eastbound in Core.");
// Clip a traffic car (entity 50).
const p22 = posAt(player, 22);
at(22.0, { Contact: { a: PLAYER, b: 50, point: [p22[0], p22[1] + 2.1, 0.5], normal: [0, 1, 0], impulse_ns: 6200, rel_speed: 9.5 } });
violation(22.0, PLAYER, "Collision", player);
at(22.01, { RegionDamaged: { vehicle: PLAYER, region: "FrontBumper", severity: 0.42 } });
at(22.01, { RegionDamaged: { vehicle: PLAYER, region: "Hood", severity: 0.18 } });
at(22.02, { ComponentDamaged: { vehicle: PLAYER, component: "Lights", health: 0.55 } });
at(22.02, { ComponentDamaged: { vehicle: PLAYER, component: "Radiator", health: 0.81 } });
spawn(26.0, 5, "Car", posAt(player, 31));
radio(26.0, 5, "Unit 5 deploying spikes on segment 120.");
const sp = posAt(player, 31);
police(26.0, { SpikesPlaced: { tool: 1, racer: PLAYER, a: off(sp, -3.5, 0), b: off(sp, 3.5, 0), sight_m: 140 } });
level(28.0, "L3");
// Side swipe by unit 4, which attempted a PIT.
police(29.6, { PitAttempt: { unit: 4, target: PLAYER } });
radio(29.6, 4, "Unit 4 attempting PIT on racer #1.");
const p30 = posAt(player, 30);
at(30.0, { Contact: { a: 1004, b: PLAYER, point: [p30[0] - 0.9, p30[1], 0.6], normal: [1, 0, 0], impulse_ns: 9800, rel_speed: 7.1 } });
at(30.01, { RegionDamaged: { vehicle: PLAYER, region: "LeftDoorF", severity: 0.55 } });
at(30.01, { RegionDamaged: { vehicle: PLAYER, region: "FrontLeftFender", severity: 0.4 } });
at(30.02, { GlassShattered: { vehicle: PLAYER, region: "LeftDoorF" } });
at(30.03, { ComponentDamaged: { vehicle: PLAYER, component: "SuspensionFL", health: 0.62 } });
at(30.03, { ComponentDamaged: { vehicle: PLAYER, component: "SteeringRack", health: 0.78 } });
police(31.0, { UnitDisabled: { unit: 4, cause: "RacerContact" } });
radio(31.0, 4, "Unit 4 out of service.");
police(40.0, { HeloLaunched: { unit: 6, target: PLAYER } });
spawn(40.0, 6, "Helicopter", off(posAt(player, 40), -500, -500));
radio(40.1, null, "Dispatch: air unit requested, ETA 20 s.");
level(42.0, "L4");
police(56.0, { ToolRemoved: { tool: 1 } });
radio(56.0, 5, "Unit 5 out of service.");
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
level(68.0, "L5");
spawn(69.0, 7, "Car", off(posAt(player, 80), 0, 0));
spawn(69.0, 8, "Car", off(posAt(player, 80), 4, 0));
police(69.0, { RoadblockPlaced: { tool: 2, racer: PLAYER, pos: posAt(player, 80), sight_m: 150 } });
radio(69.0, 7, "Unit 7 setting up roadblock on segment 88.");
at(76.0, { ComponentDamaged: { vehicle: PLAYER, component: "Engine", health: 0.66 } });
police(87.5, { PursuitEnded: { racer: PLAYER, outcome: "Escaped" } });
level(87.5, "None");
radio(87.6, null, "Dispatch: racer #1 lost. All units resume patrol.");
police(88.0, { ToolRemoved: { tool: 2 } });
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
