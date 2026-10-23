// Crash effects end-to-end check (lead, owner row 69; docs/integration/crash-fx.md).
// Usage: node e2e/crash_fx.mjs   (needs `npm run wasm`; own Vite + headless Chromium)
// Env: FX_SHOTS=dir writes the docs screenshots (docs/media/fx/).
//
// Checks:
//  1. A scripted head-on crash (crash/head_on.ron, at night) throws sparks, glass shards and
//     debris chips; the effects add at most 5 draw calls.
//  2. A Simulation-mode barrier crash (engine / radiator damaged, total loss) makes smoke.
//  3. A burnout (RWD muscle car, TCS off, full throttle from rest) lays skid marks and tyre
//     smoke; the marks sit on the road and stay after the car leaves.
//  4. Zero allocation in steady state: the sampling heap profiler sees (almost) nothing
//     allocated by src/render/fx/ while sparks, smoke and skids are live.
//  5. setQuality("low") clamps the live counts to the low caps; "high" restores the caps.
//  6. No page errors.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { chromiumPath } from "./chromium.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = Number(process.env.PORT ?? 5207);
const shots = process.env.FX_SHOTS;
if (shots) mkdirSync(shots, { recursive: true });

// Own process group, so the exit handler also stops the vite process npx starts.
const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe", detached: true });
process.on("exit", () => {
  try {
    process.kill(-server.pid, "SIGTERM");
  } catch {}
});
for (let i = 0; ; i++) {
  try {
    if ((await fetch(`http://localhost:${PORT}/`)).ok) break;
  } catch {}
  if (i > 150) throw new Error("vite did not start");
  await new Promise((r) => setTimeout(r, 200));
}

let failures = 0;
const check = (ok, msg) => {
  console.log(`${ok ? "PASS" : "FAIL"}  ${msg}`);
  if (!ok) failures++;
};

const browser = await chromium.launch({
  executablePath: chromiumPath(),
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
// Full resolution + high effects for readable screenshots (the perf check covers the default
// software-GL profile, where effects run at "low").
const page = await (await browser.newContext({ viewport: { width: 1280, height: 720 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
await page.goto(`http://localhost:${PORT}/?gfx=high&fx=high`);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 90000 });

const shot = async (name) => shots && page.screenshot({ path: join(shots, name) });
const stats = () => page.evaluate(() => window.__wp.fx.stats());
/** Clean frame for screenshots: no HUD, no debug arrows / markers. */
await page.evaluate(() => {
  document.getElementById("overlay").style.display = "none";
  for (const l of ["contacts", "cgVelocity", "tireForces", "slipColour"]) window.__wp.toggleLayer(l);
  // Count Contact events (the effects read the same drained list).
  const wp = window.__wp;
  window.__contacts = 0;
  const drain = wp.sim.eventsDrain;
  wp.sim.eventsDrain = () => {
    const ev = drain();
    for (const e of ev) if (e.Contact) window.__contacts++;
    return ev;
  };
});
/** Run a scenario file; `hour` sets the time of day (night shots), `kmh` the cruise speed. */
const runScenario = (file, kmh, hour) =>
  page.evaluate(
    async ([file, kmh, hour]) => {
      const wp = window.__wp;
      const withSpeed = wp.sim.scenarioWithSpeed;
      if (hour !== null) wp.sim.scenarioWithSpeed = (json, mps) => JSON.stringify({ ...JSON.parse(withSpeed(json, mps)), time_of_day_h: hour });
      window.__contacts = 0;
      try {
        await wp.runScenario(file, kmh);
      } finally {
        wp.sim.scenarioWithSpeed = withSpeed;
      }
    },
    [file, kmh, hour ?? null],
  );
/** Point the orbit camera at entity `id` from an offset (sim coordinates, m). */
const frame = (id, eye, at) =>
  page.evaluate(
    ([id, eye, at]) => {
      const p = window.__wp.app.views.get(id).root.position;
      window.__wp.lookFrom([p.x + eye[0], p.y + eye[1], p.z + eye[2]], [p.x + at[0], p.y + at[1], p.z + at[2]]);
    },
    [id, eye, at],
  );

// ----- 1. Head-on crash at night: sparks, glass, debris --------------------------------------
await runScenario("crash/head_on.ron", 80, 23);
await frame(1, [7, 6, 2.2], [0, 8, 0.6]);
await page.waitForFunction(() => window.__contacts > 0, null, { timeout: 30000, polling: 16 });
await frame(1, [6.5, 8.5, 2.0], [0, 2.5, 0.6]);
await page.waitForTimeout(40);
await shot("crash_sparks_night.png");
await page.waitForTimeout(1500);
let s = await stats();
check(s.sparks >= 20, `head-on crash throws sparks (${s.sparks})`);
check(s.shards >= 10, `head-on crash shatters glass into shards (${s.shards})`);
check(s.chips >= 5, `head-on crash scatters debris chips (${s.chips})`);
check(s.drawCalls <= 5, `effects add at most 5 draw calls (${s.drawCalls})`);
// Bits land and stay a while (rest on the ground, still alive seconds later).
await frame(1, [5, 7, 3.2], [0, 3, 0]);
await page.waitForTimeout(1500);
s = await stats();
check(s.alive.shards > 0 && s.alive.chips > 0, `glass and debris stay on the ground (${s.alive.shards} shards, ${s.alive.chips} chips alive)`);
// Every live bit sits at or above its ground plane (no piece sinks into the road).
const rest = await page.evaluate(() => window.__wp.fx.belowGround());
check(rest === 0, `no glass or debris below the road (${rest} below)`);
await shot("crash_glass_debris_night.png");

// ----- 2. Simulation barrier crash: engine / radiator smoke ---------------------------------
await runScenario("crash/barrier_frontal_sim.ron", 56, null);
await page.waitForFunction(() => window.__contacts > 0, null, { timeout: 30000, polling: 50 });
await page.waitForTimeout(2500);
await frame(1, [-6.5, -4, 3], [0, 1, 1.2]);
await page.waitForTimeout(400);
s = await stats();
check(s.smoke >= 10 && s.alive.smoke > 0, `damaged engine / radiator smokes (${s.smoke} puffs, ${s.alive.smoke} alive)`);
await shot("engine_smoke.png");

// ----- 3. Burnout: skid marks + tyre smoke ---------------------------------------------------
await page.evaluate(() => {
  window.__wp.startFreeDrive("ironside_440", "Arcade", false);
});
await page.keyboard.down("KeyW");
await page.waitForTimeout(3200);
// On a slow software-GL box 3.2 s of wall time can cover less sim time (frames over 250 ms
// lose time): give the burnout up to 5 s more to reach the count instead of a fixed sleep.
await page.waitForFunction(() => window.__wp.fx.stats().skidSegments >= 8, null, { timeout: 5000, polling: 100 }).catch(() => {});
s = await stats();
check(s.skidSegments >= 8, `burnout lays skid marks (${s.skidSegments} segments)`);
check(s.smoke >= 10, `burnout makes tyre smoke (${s.smoke} puffs)`);
await frame(1, [5, -7, 2.6], [0, -4, 0.3]);
await page.waitForTimeout(150);
await shot("burnout_skids.png");

// ----- 4. Zero allocation in steady state ----------------------------------------------------
const cdp = await page.context().newCDPSession(page);
await cdp.send("HeapProfiler.enable");
await cdp.send("HeapProfiler.collectGarbage");
// Drive the effects directly (same code path as the frame loop: the live car's states, its
// view, an empty event list) so the JIT has warmed up; headless frames are too slow for that.
const FX_FRAMES = 600;
const runFx = (n) =>
  page.evaluate((n) => {
    const wp = window.__wp;
    const cur = wp.sim.states();
    for (let i = 0; i < n; i++) wp.fx.frame([], cur, wp.app.views, 1 / 60, 0);
  }, n);
await runFx(FX_FRAMES);
await cdp.send("HeapProfiler.startSampling", { samplingInterval: 256, includeObjectsCollectedByMajorGC: true, includeObjectsCollectedByMinorGC: true });
await runFx(FX_FRAMES);
const { profile } = await cdp.send("HeapProfiler.stopSampling");
const frames = FX_FRAMES;
let fxBytes = 0;
const fxFns = new Map();
const walk = (n) => {
  if (n.callFrame.url.includes("/render/fx/")) {
    fxBytes += n.selfSize;
    const k = `${n.callFrame.functionName || "(anon)"} ${n.callFrame.url.split("/").pop().split("?")[0]}:${n.callFrame.lineNumber + 1}:${n.callFrame.columnNumber + 1}`;
    fxFns.set(k, (fxFns.get(k) ?? 0) + n.selfSize);
  }
  for (const c of n.children) walk(c);
};
walk(profile.head);
s = await stats();
const fxKBPerFrame = fxBytes / 1024 / Math.max(frames, 1);
check(
  s.alive.smoke > 0 && s.alive.skids > 0 && fxKBPerFrame < 1,
  `effects allocate ${fxKBPerFrame.toFixed(2)} KB/frame in steady state (< 1; ${frames} frames, ${s.alive.smoke} puffs, ${s.alive.skids} skid segments live)` +
    (fxFns.size ? ` top: ${[...fxFns].sort((a, b) => b[1] - a[1]).slice(0, 3).map(([k, v]) => `${k} ${v} B`).join(", ")}` : ""),
);
// Report the CPU cost of one effects frame at "high" with this much live (perf doc).
const fxMs = await page.evaluate((n) => {
  const wp = window.__wp;
  const cur = wp.sim.states();
  const t0 = performance.now();
  for (let i = 0; i < n; i++) wp.fx.frame([], cur, wp.app.views, 1 / 60, 0);
  return (performance.now() - t0) / n;
}, FX_FRAMES);
s = await stats();
console.log(`INFO  effects frame at high: ${fxMs.toFixed(3)} ms (${s.alive.sparks} sparks, ${s.alive.smoke} puffs, ${s.alive.shards} shards, ${s.alive.chips} chips, ${s.alive.skids} skid segments live)`);
await page.keyboard.up("KeyW");

// The marks stay where they were laid after the car has gone.
await page.keyboard.down("KeyW");
await page.waitForTimeout(1500);
await page.keyboard.up("KeyW");
s = await stats();
check(s.alive.skids >= 8, `skid marks stay behind the car (${s.alive.skids} segments)`);

// ----- 3b. Skid marks on a hill (Port Vesper v2 spawn: ~12 m up, ~7 % grade) -----------------
await page.evaluate(() => window.__wp.startFreeDrive("ironside_440", "Arcade", true));
// The city greybox parses for a few seconds under SwiftShader: wait for the sim to run.
await page.waitForFunction(() => window.__wp.sim.worldInfo()[0] > 240, null, { timeout: 60000 });
const start = await page.evaluate(() => {
  const r = window.__wp.app.views.get(window.__wp.app.player).root;
  const f = { x: 0, y: 1, z: 0 };
  const q = r.quaternion;
  // Forward (+Y) rotated by the car's quaternion.
  const fx = 2 * (q.x * q.y - q.w * q.z);
  const fy = 1 - 2 * (q.x * q.x + q.z * q.z);
  const fz = 2 * (q.y * q.z + q.w * q.x);
  return { x: r.position.x, y: r.position.y, z: r.position.z, fx, fy, fz, f };
});
await page.keyboard.down("KeyW");
await page.waitForFunction(() => window.__wp.fx.skids.segments >= 10, null, { timeout: 30000 }).catch(() => {});
await page.keyboard.up("KeyW");
const hill = await page.evaluate(() => {
  const fx = window.__wp.fx;
  const pos = fx.skids.mesh.geometry.attributes.position.array;
  const n = fx.skids.segments * 4;
  let minZ = Infinity;
  let maxZ = -Infinity;
  let off = 0;
  const plane = fx.cars.get(window.__wp.app.player)?.plane;
  for (let v = 0; v < n; v++) {
    const z = pos[v * 3 + 2];
    minZ = Math.min(minZ, z);
    maxZ = Math.max(maxZ, z);
    if (plane) off = Math.max(off, Math.abs(z - (plane.ga + plane.gb * pos[v * 3] + plane.gc * pos[v * 3 + 1])));
  }
  return { segments: fx.skids.segments, minZ, maxZ, off };
});
check(
  // On the road up the hill (not at z = 0), climbing with it, and close to the plane the
  // car's wheels define now (fitted at the car, pitched by the burnout: a loose bound).
  hill.segments >= 4 && hill.minZ > 5 && hill.maxZ - hill.minZ > 0.03 && hill.off < 0.25,
  `skid marks follow the hill road (${hill.segments} segments climbing z ${hill.minZ.toFixed(2)}–${hill.maxZ.toFixed(2)} m, ≤ ${hill.off.toFixed(2)} m off the car's wheel-contact plane)`,
);
await page.evaluate((s) => {
  // Behind and beside the start, looking up the road along the marks.
  window.__wp.lookFrom([s.x - s.fx * 5 + s.fy * 2.5, s.y - s.fy * 5 - s.fx * 2.5, s.z + 2.2], [s.x + s.fx * 6, s.y + s.fy * 6, s.z + s.fz * 6]);
}, start);
await page.waitForTimeout(200);
await shot("skids_hill_city.png");

// ----- 5. Quality switch ---------------------------------------------------------------------
const q = await page.evaluate(() => {
  const fx = window.__wp.fx;
  fx.setQuality("low");
  const low = fx.stats();
  const caps = [fx.sparks.pool.cap, fx.smoke.pool.cap, fx.shards.pool.cap, fx.chips.pool.cap, fx.skids.cap];
  fx.setQuality("high");
  return { low, caps, high: [fx.sparks.pool.cap, fx.smoke.pool.cap, fx.shards.pool.cap, fx.chips.pool.cap, fx.skids.cap], level: fx.quality };
});
check(
  q.low.quality === "low" && q.low.alive.smoke <= q.caps[1] && q.caps.join() === "160,96,96,128,2048" && q.high.join() === "800,320,384,640,12288" && q.level === "high",
  `setQuality low/high clamps pools (low caps ${q.caps.join("/")}, high ${q.high.join("/")})`,
);

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
console.log(failures === 0 ? "\ncrash fx checks passed" : `\n${failures} crash fx check(s) FAILED`);
process.exit(failures === 0 ? 0 : 1);
