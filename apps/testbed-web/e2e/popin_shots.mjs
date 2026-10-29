// Traffic pop-in evidence for docs/media/popin/ (lead/car-popin, owner report "cars also render
// in free roam too close"): chase-camera shots in Free Roam (combined world) on a long Port Vesper
// road (downtown avenue) and on the Ashpeak highway (Route 7), day and night, with every traffic car that
// appeared or vanished inside the camera's view marked, plus frame times. Works against this
// build and an older testbed (only uses hooks main already had), so the same script makes the
// "before" and "after" sets.
//
// Usage (after `npm run wasm`): node e2e/popin_shots.mjs <outDir> <label> [testbedDir]
// e.g. node e2e/popin_shots.mjs ../../docs/media/popin after
//      node e2e/popin_shots.mjs ../../docs/media/popin before /path/to/main-worktree/apps/testbed-web
// then  cargo test -p wp_world --release --test popin -- --ignored classify_browser_popins
// then  node e2e/popin_compose.mjs ../../docs/media/popin before after
// Env: CHROMIUM=/path/to/chrome, GFX=high|low|auto (default high: the PC profile, full draw
// distance), ONLY=<shot id> (one shot), RAW=<dir> (raw frames; default <outDir>/raw, not
// committed), CHECK=1 (exit 1 if a traffic car within the draw distance was not drawn).
//
// The browser only knows a car was inside the camera's view cone; whether it was in plain view
// or behind a building is decided by the line-of-sight check on the same map (wp_world
// tests/popin.rs `classify_browser_popins`, which writes `<label>_los.json`).
//
// Output: `<label>_summary.json` (per shot: traffic census, every car that appeared or vanished
// inside the view cone with its position, the camera position and the raw frames around it,
// whether traffic within the draw distance went undrawn, frame times) and raw frames.
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const out = resolve(process.argv[2] ?? join(here, "../../../docs/media/popin"));
const label = process.argv[3] ?? "after";
const root = resolve(process.argv[4] ?? join(here, ".."));
const GFX = process.env.GFX ?? "high";
const CHECK = process.env.CHECK === "1";
const raw = resolve(process.env.RAW ?? join(out, "raw"));
/** wp_world `TrafficConfig::view_radius_m`: no car may appear or vanish on screen closer. */
const VIEW_RADIUS_M = 1000;
mkdirSync(out, { recursive: true });
const PORT = label === "before" ? 5196 : 5197;
// `?roam=10`: the testbed opens in Free Roam (combined world, 10 AI street racers roaming).
const URL = `http://localhost:${PORT}/?gfx=${GFX}&roam=10`;

/** Places: a sim position on the road, heading (yaw, 0 = +Y) and start speed. */
const PLACES = [
  // Downtown Port Vesper, the long north-south avenue at x ≈ 0 southbound: ~980 m of clear
  // sightline down the street, the busiest district (Loop 9 has a better view but almost no
  // traffic, 5-8 cars in reach).
  { id: "vesper_downtown", label: "Port Vesper · downtown avenue", pos: [-5, 140], yaw: 3.1416, speed: 13 },
  // Route 7 southbound north of Ashpeak City: ~1060 m of clear sightline.
  { id: "ashpeak_route7", label: "Ashpeak · Route 7", pos: [507, 5339], yaw: -3.084, speed: 25 },
];
const TIMES = [
  { id: "day", hour: 13 },
  { id: "night", hour: 22 },
];

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; i < 150; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
  await new Promise((r) => setTimeout(r, 200));
}

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1243/chrome-linux64/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1280, height: 720 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 240000 });
await page.addStyleTag({ content: "#overlay, #hud, .hud, #help { display: none !important; }" });

// In-page helpers (build-independent).
await page.evaluate(() => {
  const wp = window.__wp;
  const AG = 7; // traffic_agents(): [x, y, z, yaw, flags, agentId, model]
  const frames = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  window.__pop = {
    frames,
    async start(hour, place) {
      // Restart the Free Roam session (its own start path: roamers are AI racers, street
      // racers join) at `hour` with the clock nearly stopped.
      const init = wp.sim.init;
      wp.sim.init = (seed, json, scripted) => init(seed, JSON.stringify({ ...JSON.parse(json), time_of_day_h: hour, time_ratio: 0.01 }), scripted);
      try {
        wp.restartRun();
      } finally {
        wp.sim.init = init;
      }
      if (wp.sim.scenario().name !== "free_roam") throw new Error(`not free roam: ${wp.sim.scenario().name}`);
      wp.app.paused = true;
      wp.setCamera("chase");
      for (let i = 0; i < 600; i++) {
        if ([...wp.scene.cities.values()].some((c) => c.children.length > 0)) break;
        await new Promise((r) => setTimeout(r, 200));
      }
      this.place(place, 0);
      await frames();
    },
    place(p, speed) {
      wp.sim.debugPlace(wp.app.player, p.pos[0], p.pos[1], p.yaw, speed);
    },
    /** Step the sim; render a frame only with `render` (software GL: ~1 s a frame). */
    async step(ticks, render = true) {
      wp.sim.step(ticks / 240);
      if (render) await frames();
    },
    /** Sim steps without rendering (warm-up). */
    run(seconds) {
      for (let i = 0; i < seconds * 4; i++) wp.sim.step(0.25);
    },
    player() {
      const v = wp.app.views.get(wp.app.player);
      const p = v.root.position;
      return [p.x, p.y, p.z];
    },
    /** Traffic agents with their screen position (null = outside the view frustum). */
    agents() {
      const a = wp.sim.trafficAgents();
      const cam = wp.rig.camera;
      cam.updateMatrixWorld();
      const V = cam.position.constructor;
      const pl = this.player();
      const c = cam.getWorldPosition(new V());
      // Camera in sim space (for the offline line-of-sight check, wp_world tests/popin.rs).
      this.camSim = [+c.x.toFixed(2), +(-c.z).toFixed(2), +c.y.toFixed(2)];
      const out = [];
      for (let o = 0; o < a.length; o += AG) {
        const x = a[o], y = a[o + 1], z = a[o + 2];
        // Sim (x, y, z) → Three (x, z, -y); `world` is rotated -90° about X.
        const v = new V(x, z + 0.8, -y).project(cam);
        const on = v.z > -1 && v.z < 1 && Math.abs(v.x) <= 1 && Math.abs(v.y) <= 1;
        out.push({
          id: a[o + 5],
          pos: [x, y, z],
          dist: Math.hypot(x - pl[0], y - pl[1]),
          screen: on ? [((v.x + 1) / 2) * innerWidth, ((1 - v.y) / 2) * innerHeight] : null,
        });
      }
      return out;
    },
    /** City cull distance this frame (draw distance, far plane, fog), m. */
    cull() {
      return wp.scene.cullDistanceM(wp.rig.camera);
    },
    /** Traffic cars within `maxM` of the camera the LOD director did not draw (none / culled). */
    undrawn(maxM) {
      if (!wp.lod?.states) return null;
      return wp.lod.states.filter((s) => s.key.startsWith("t") && s.dist < maxM && (s.mode === "none" || s.mode === "culled")).map((s) => ({ key: s.key, mode: s.mode, dist: Math.round(s.dist) }));
    },
    /** Live frame times for `ms` (rAF intervals; the app running unpaused). */
    async frameTimes(ms) {
      wp.app.paused = false;
      const t = [];
      let last = performance.now();
      const end = last + ms;
      while (performance.now() < end) {
        await new Promise((r) => requestAnimationFrame(r));
        const now = performance.now();
        t.push(now - last);
        last = now;
      }
      wp.app.paused = true;
      t.sort((a, b) => a - b);
      const mean = t.reduce((s, x) => s + x, 0) / t.length;
      return { frames: t.length, meanMs: +mean.toFixed(1), p95Ms: +t[Math.floor(t.length * 0.95)].toFixed(1), fps: +(1000 / mean).toFixed(1) };
    },
  };
});

const summary = { label, gfx: GFX, viewport: "1280x720", shots: [] };
let checkFail = 0;
const shot = () => page.screenshot({ type: "png" });

for (const place of PLACES) {
  for (const time of TIMES) {
    const id = `${place.id}_${time.id}`;
    if (process.env.ONLY && process.env.ONLY !== id) continue;
    await page.evaluate(([h, p]) => window.__pop.start(h, p), [time.hour, place]);
    // The session starts on the Port Vesper grid: the player is moved to the spot and traffic
    // fills in around it the way it does in play (40 s), then the player drives off.
    await page.evaluate(() => window.__pop.run(40));
    await page.evaluate((p) => window.__pop.place(p, 0), place);
    await page.evaluate(() => window.__pop.run(4));
    await page.evaluate((p) => window.__pop.place(p, p.speed), place);
    await page.evaluate(() => window.__pop.step(4));
    const cull = await page.evaluate(() => window.__pop.cull());
    let last = await page.evaluate(() => window.__pop.agents());
    const band = (l, lo, hi) => l.filter((a) => a.dist >= lo && a.dist < hi).length;
    const census = (l) => ({ total: l.length, onScreen: l.filter((a) => a.screen).length, "<150": band(l, 0, 150), "150-320": band(l, 150, 320), "320-600": band(l, 320, 600), "600-1000": band(l, 600, 1000), ">1000": band(l, 1000, 1e9) });
    const startCensus = census(last);
    // Raw frames (PNG, not committed): every strip frame and the frame pair around each
    // on-screen pop-in, for e2e/popin_compose.mjs.
    const rawDir = join(raw, `${label}_${id}`);
    mkdirSync(rawDir, { recursive: true });
    const save = async (step) => writeFileSync(join(rawDir, `f${step}.png`), await shot());
    await save(0);
    let lastFrame = 0;
    const appeared = [];
    const vanished = [];
    const undrawn = [];
    const strip = [];
    // 6 s at 60 steps/s (one traffic step, 1/60 s, each checked for cars that appeared or
    // vanished on screen), rendering every 10th step (the camera follows), on every pop-in, and
    // a strip frame every second.
    for (let i = 1; i <= 360; i++) {
      await page.evaluate((r) => window.__pop.step(4, r), i % 10 === 0);
      const now = await page.evaluate(() => window.__pop.agents());
      const was = new Map(last.map((a) => [a.id, a]));
      const is = new Map(now.map((a) => [a.id, a]));
      const newOnScreen = now.filter((a) => !was.has(a.id) && a.screen && a.dist < cull);
      const goneOnScreen = last.filter((a) => !is.has(a.id) && a.screen && a.dist < cull);
      const pop = newOnScreen.length + goneOnScreen.length > 0;
      if (pop && i % 10 !== 0) await page.evaluate(() => window.__pop.frames());
      const cam = await page.evaluate(() => window.__pop.camSim);
      const ev = (a) => ({ step: i, id: a.id, dist: Math.round(a.dist), pos: a.pos.map((x) => +x.toFixed(2)), cam, screen: a.screen.map((x) => Math.round(x)), before: lastFrame, frame: i });
      for (const a of newOnScreen) appeared.push(ev(a));
      for (const a of goneOnScreen) vanished.push(ev(a));
      if (pop || i % 10 === 0) {
        await save(i);
        lastFrame = i;
      }
      if (i % 60 === 0) {
        const u = await page.evaluate((m) => window.__pop.undrawn(m), cull - 5);
        if (u) undrawn.push(...u);
        strip.push(i);
      }
      last = now;
    }
    const frameTimes = await page.evaluate(() => window.__pop.frameTimes(6000));
    const near = (l) => l.filter((a) => a.dist < VIEW_RADIUS_M);
    const s = {
      id,
      place: place.label,
      hour: time.hour,
      cullDistanceM: Math.round(cull),
      trafficAtStart: startCensus,
      trafficAtEnd: census(last),
      appearedInViewCone: appeared.length,
      appearedInViewConeWithinViewRadius: near(appeared).length,
      nearestAppearM: appeared.length ? Math.min(...appeared.map((a) => a.dist)) : null,
      vanishedInViewCone: vanished.length,
      vanishedInViewConeWithinViewRadius: near(vanished).length,
      nearestVanishM: vanished.length ? Math.min(...vanished.map((a) => a.dist)) : null,
      undrawnWithinCull: undrawn,
      frameTimes,
      strip,
      appeared,
      vanished,
    };
    summary.shots.push(s);
    console.log(`${id}: ${JSON.stringify({ ...s, appeared: undefined, vanished: undefined, strip: undefined })}`);
    if (CHECK && undrawn.length > 0) checkFail++;
  }
}

summary.pageErrors = errors;
writeFileSync(join(out, `${label}_summary.json`), JSON.stringify(summary, null, 1) + "\n");
await browser.close();
server.kill("SIGTERM");
if (errors.length) console.log("page errors:", errors);
if (CHECK && checkFail > 0) {
  console.log(`FAIL  ${checkFail} shot(s) had traffic within the draw distance that was not drawn`);
  process.exit(1);
}
process.exit(0);
