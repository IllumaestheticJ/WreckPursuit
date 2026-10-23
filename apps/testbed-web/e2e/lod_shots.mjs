// LOD pop-in evidence for docs/media/lod/ (lead helper, playtest row 25): screenshot strips and
// frame-by-frame draw logs of traffic cars around a promotion, plus a look down the road ahead.
// Works against this build and against an older testbed (only uses hooks `main` already had), so
// the same script makes the "before" and "after" sets.
//
// Usage (after `npm run wasm`): node e2e/lod_shots.mjs <outDir> <label> [testbedDir]
// e.g. node e2e/lod_shots.mjs ../../docs/media/lod after
//      node e2e/lod_shots.mjs ../../docs/media/lod before /path/to/main-worktree/apps/testbed-web
// Env: CHROMIUM=/path/to/chrome, CAR=<player car> (default lindwurm_4t, the testbed default).
//
// Output: <label>_promotion_strip.png (camera above one car as it is promoted to a physical
// vehicle), <label>_ahead_strip.png (camera behind the player looking 300 m down the road),
// <label>_frames.json (per step: the followed car's draw state; mesh load timings; for builds
// with the LOD log, its summary and swap windows).
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const out = resolve(process.argv[2] ?? join(here, "../../../docs/media/lod"));
const label = process.argv[3] ?? "after";
const root = resolve(process.argv[4] ?? join(here, ".."));
mkdirSync(out, { recursive: true });
const PORT = label === "before" ? 5194 : 5195;
const URL = `http://localhost:${PORT}/`;

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; i < 150; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
  await new Promise((r) => setTimeout(r, 200));
}

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1280, height: 720 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 180000 });
await page.addStyleTag({ content: "#overlay { display: none !important; }" });

// Night run in Port Vesper v2 (night, police, traffic), player already at 30 m/s. Let it run
// live for a moment, then pause and step the sim by hand so every frame is captured.
// The testbed's default car (the owner's playtest car); CAR=… overrides.
await page.evaluate((car) => window.__wp.startNightRun(car, "Arcade"), process.env.CAR ?? "lindwurm_4t");
await page.waitForFunction(() => (window.__wp.scene.cities.get("vesper_v2")?.children.length ?? 0) > 0, null, { timeout: 90000 });
await page.keyboard.down("KeyW");
await page.waitForFunction(() => window.__wp.sim.worldInfo()[0] / 240 > 2, null, { timeout: 120000, polling: 200 });
if (await page.evaluate(() => !!window.__wp.lod)) await page.evaluate(() => window.__wp.lod.log.start(window.__wp.sim.worldInfo()[0] / 240));

// In-page helpers: manual stepping and a build-independent description of how the car nearest
// a point is drawn.
await page.evaluate(() => {
  const wp = window.__wp;
  wp.app.paused = true;
  const frames = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  const AG = 7; // traffic_agents(): [x, y, z, yaw, flags, agentId, model]
  window.__lodShots = {
    async step(ticks) {
      wp.sim.step(ticks / 240);
      await frames();
    },
    agents() {
      const a = wp.sim.trafficAgents();
      const out = [];
      for (let o = 0; o < a.length; o += AG) out.push({ pos: [a[o], a[o + 1], a[o + 2]], yaw: a[o + 3], flags: a[o + 4] });
      return out;
    },
    player() {
      const v = wp.app.views.get(wp.app.player);
      const p = v.root.position;
      return [p.x, p.y, p.z];
    },
    /** How the car at `pos` is drawn this frame. */
    describe(pos, flags) {
      const near = (p) => Math.hypot(p.x - pos[0], p.y - pos[1]);
      let view = null;
      let best = 4;
      for (const v of wp.app.views.values()) {
        if (v.info.kind !== "Traffic") continue;
        const d = near(v.root.position);
        if (d < best) [view, best] = [v, d];
      }
      const physical = (flags & 1) !== 0;
      if (wp.lod) {
        // This build: the LOD director's own state for the nearest car.
        let st = null;
        let bd = 4;
        for (const s of wp.lod.states) {
          const d = Math.hypot(s.pos[0] - pos[0], s.pos[1] - pos[1]);
          if (s.key.startsWith("t") && d < bd) [st, bd] = [s, d];
        }
        return {
          physical,
          drawer: st ? (st.mode === "view" ? "VehicleView (panel mesh)" : `instanced ${st.mode} mesh`) : "nothing",
          key: st?.key ?? null,
          drawnPos: st?.pos ?? null,
          colour: view ? "#" + view.panels.paint.color.getHexString() : null,
        };
      }
      // Older build (main before the fix): scene.updateAgents(trafficAgents, skipPhysical) drew
      // a 1.8 × 4.4 × 1.4 m box per agent, skipping physical agents whenever any physical traffic
      // vehicle existed; physical agents still drawn as boxes were yellow.
      const meta = wp.sim.vehicleMeta();
      let anyTraffic = false;
      for (let o = 0; o < meta.length; o += 3) anyTraffic ||= meta[o + 1] === 3;
      const boxDrawn = !(anyTraffic && physical);
      const boxColour = flags & 8 ? "#3399ff (yielding)" : flags & 2 ? "#ff5040 (braking)" : physical ? "#ffd040 (physical)" : "#b8bcc4";
      const viewDraw = view ? (view.panels.ready ? "VehicleView (panel mesh)" : "VehicleView placeholder (mesh still loading)") : null;
      return {
        physical,
        drawer: [boxDrawn ? `box ${boxColour}` : null, viewDraw].filter(Boolean).join(" + ") || "nothing",
        boxCentreZ: boxDrawn ? pos[2] + 0.7 : null,
        viewPos: view ? [view.root.position.x, view.root.position.y, view.root.position.z] : null,
        colour: view ? "#" + view.panels.paint.color.getHexString() : null,
      };
    },
  };
});

const shot = async () => page.screenshot({ type: "png", clip: { x: 320, y: 160, width: 640, height: 400 } });

// 1. Promotion strip: follow a kinematic car approaching the player from 90-120 m with a camera
// above and behind it. Promotion happens at 80 m.
const rows = [];
const frames = [];
let target = null;
let promotedAt = -1;
// Pop-in distances (any build): cars with no car within 2 m of them on the previous step
// appeared; cars with none on the next step vanished. Steps are 1/60 s (< 1 m of motion).
const pops = { appear: [], vanish: [] };
let lastAgents = null;
let lastPlayer = null;
const unmatched = (a, b) => a.filter((x) => !b.some((y) => Math.hypot(x.pos[0] - y.pos[0], x.pos[1] - y.pos[1]) < 2));
for (let i = 0; i < 1800 && (promotedAt < 0 || i < promotedAt + 8); i++) {
  await page.evaluate(() => window.__lodShots.step(4)); // one traffic step (1/60 s) per frame
  const { agents, player } = await page.evaluate(() => ({ agents: window.__lodShots.agents(), player: window.__lodShots.player() }));
  const dist = (p) => Math.hypot(p[0] - player[0], p[1] - player[1]);
  if (lastAgents) {
    for (const a of unmatched(agents, lastAgents)) pops.appear.push(+dist(a.pos).toFixed(1));
    for (const a of unmatched(lastAgents, agents)) pops.vanish.push(+Math.hypot(a.pos[0] - lastPlayer[0], a.pos[1] - lastPlayer[1]).toFixed(1));
  }
  lastAgents = agents;
  lastPlayer = player;
  if (!target) {
    const c = agents.find((a) => !(a.flags & 1) && dist(a.pos) > 88 && dist(a.pos) < 120);
    if (!c) continue;
    target = { pos: c.pos, yaw: c.yaw, flags: c.flags, prevDist: dist(c.pos), start: i };
    continue;
  }
  // Track by nearest position (both builds), drop it if it moves away from the player.
  let best = null;
  let bd = 6;
  for (const a of agents) {
    const d = Math.hypot(a.pos[0] - target.pos[0], a.pos[1] - target.pos[1]);
    if (d < bd) [best, bd] = [a, d];
  }
  if (!best || (promotedAt < 0 && i - target.start > 360)) {
    target = null;
    frames.length = 0;
    rows.length = 0;
    continue;
  }
  target = { ...target, pos: best.pos, yaw: best.yaw, flags: best.flags };
  // Car frame: forward (-sin, cos), right (cos, sin). Eye 10 m behind, 4 m right, 14 m up
  // (clear of walls and terrain on the hill roads).
  const fwd = [-Math.sin(best.yaw), Math.cos(best.yaw)];
  const right = [Math.cos(best.yaw), Math.sin(best.yaw)];
  const eye = [best.pos[0] + right[0] * 4 - fwd[0] * 10, best.pos[1] + right[1] * 4 - fwd[1] * 10, best.pos[2] + 14];
  await page.evaluate(([e, t]) => window.__wp.lookFrom(e, t), [eye, [best.pos[0], best.pos[1], best.pos[2] + 0.8]]);
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  const d = await page.evaluate(([p, f]) => window.__lodShots.describe(p, f), [best.pos, best.flags]);
  rows.push({ step: i, simS: +(await page.evaluate(() => window.__wp.sim.worldInfo()[0] / 240)).toFixed(3), distToPlayer: +dist(best.pos).toFixed(1), agentPos: best.pos.map((x) => +x.toFixed(3)), ...d });
  frames.push({ png: await shot(), caption: `${Math.round(((i - target.start) * 1000) / 60)} ms · ${dist(best.pos).toFixed(0)} m · ${d.physical ? "PHYSICAL" : "kinematic"}`, physical: d.physical });
  if (frames.length > 14 && promotedAt < 0) {
    frames.shift();
    rows.shift();
  }
  if (promotedAt < 0 && d.physical) promotedAt = i;
}

// 2. Ahead strip: back behind the player, looking 300 m down the road, one frame every 0.5 s.
await page.evaluate(() => window.__wp.setCamera("chase"));
const ahead = [];
for (let k = 0; k < 8; k++) {
  await page.evaluate(() => window.__lodShots.step(120));
  const v = await page.evaluate(() => {
    const wp = window.__wp;
    const st = wp.sim.states();
    const p = [st[1], st[2], st[3]];
    const q = [st[4], st[5], st[6], st[7]];
    // Chassis +Y in world.
    const fx = 2 * (q[0] * q[1] - q[3] * q[2]);
    const fy = 1 - 2 * (q[0] * q[0] + q[2] * q[2]);
    const n = Math.hypot(fx, fy) || 1;
    const f = [fx / n, fy / n];
    wp.lookFrom([p[0] - f[0] * 14, p[1] - f[1] * 14, p[2] + 9], [p[0] + f[0] * 300, p[1] + f[1] * 300, p[2]]);
    const a = window.__lodShots.agents();
    const within = (lo, hi) => a.filter((x) => {
      const d = Math.hypot(x.pos[0] - p[0], x.pos[1] - p[1]);
      return d >= lo && d < hi;
    }).length;
    return { near: within(0, 150), mid: within(150, 320), far: within(320, 1e9) };
  });
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  ahead.push({ png: await page.screenshot({ type: "png" }), caption: `t+${k * 0.5}s · cars <150 m: ${v.near}, 150–320 m: ${v.mid}, >320 m: ${v.far}` });
}
await page.keyboard.up("KeyW");

// Mesh fetches (both builds): resource timings of data/panels/meshes/*.json.
const meshes = await page.evaluate(() =>
  performance
    .getEntriesByType("resource")
    .filter((e) => e.name.includes("/data/panels/meshes/"))
    .map((e) => ({ car: e.name.split("/").pop().replace(".json", ""), startMs: Math.round(e.startTime), durationMs: Math.round(e.duration) })),
);
const lodLog = await page.evaluate(() =>
  window.__wp.lod
    ? {
        summary: window.__wp.lod.log.summary(),
        swaps: window.__wp.lod.log.swaps.slice(0, 12),
        meshTimings: Object.fromEntries([...window.__wp.meshTimings].map(([k, v]) => [k, { ms: v.end === null ? null : Math.round(v.end - v.start), ok: v.ok }])),
      }
    : null,
);

// Compose the strips on a blank page.
const strip = async (items, file, width) => {
  const p = await browser.newPage({ viewport: { width: 1400, height: 400 } });
  const cells = items
    .map(
      (f) =>
        `<figure style="margin:4px;width:${width}px;display:inline-block;vertical-align:top"><img style="width:${width}px;display:block;border:2px solid ${f.physical ? "#e0a020" : "#444"}" src="data:image/png;base64,${f.png.toString("base64")}"><figcaption style="font:12px monospace;color:#ddd">${f.caption}</figcaption></figure>`,
    )
    .join("");
  await p.setContent(`<body style="margin:6px;background:#111"><div style="font:14px sans-serif;color:#fff;margin:4px">${label}: ${file.replace(/_/g, " ").replace(".png", "")}</div>${cells}</body>`);
  await p.screenshot({ path: join(out, `${label}_${file}`), fullPage: true });
  await p.close();
};
// Promotion strip: 6 frames before the promotion, the swap frame and 5 after (every other step).
const pi = frames.findIndex((f) => f.physical);
const pick = pi < 0 ? frames : frames.slice(Math.max(0, pi - 6), pi + 6);
await strip(pick, "promotion_strip.png", 320);
await strip(ahead, "ahead_strip.png", 330);
writeFileSync(
  join(out, `${label}_frames.json`),
  JSON.stringify(
    {
      label,
      promotionStep: promotedAt,
      popIn: { minAppearM: Math.min(...pops.appear), minVanishM: Math.min(...pops.vanish), appeared: pops.appear.length, vanished: pops.vanish.length },
      followedCar: rows,
      meshFetches: meshes,
      lodLog,
      errors,
    },
    null,
    1,
  ),
);
console.log(`${label}: cars appeared ${pops.appear.length}× (closest ${Math.min(...pops.appear)} m), vanished ${pops.vanish.length}× (closest ${Math.min(...pops.vanish)} m)`);
console.log(`${label}: promotion at step ${promotedAt}, ${rows.length} logged frames, ${meshes.length} mesh fetches, errors ${errors.length}`);
for (const r of rows.slice(Math.max(0, rows.findIndex((r) => r.physical) - 3), rows.findIndex((r) => r.physical) + 4)) console.log(JSON.stringify(r));
if (lodLog) console.log(JSON.stringify(lodLog.summary));
await browser.close();
process.exit(0);
