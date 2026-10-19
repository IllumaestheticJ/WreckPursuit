// City-chase performance in headless Chromium: sim ms per frame and the real-time factor
// (sim seconds advanced per wall second). Usage: node e2e/chase_perf.mjs [car] [mode] [seconds]
// (needs `npm run wasm`; starts its own Vite). Prints one JSON line.
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 5198;
const URL = `http://localhost:${PORT}/`;
const [car = "", mode = "Arcade", secs = "20"] = process.argv.slice(2);

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; ; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
  if (i > 100) throw new Error("vite did not start");
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
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
await page.evaluate(([c, m]) => window.__wp.startCityChase(c || undefined, m), [car, mode]);
await page.keyboard.down("KeyW");
await page.waitForTimeout(3000); // warm-up (spawns, JIT)
const result = await page.evaluate(async (ms) => {
  const wp = window.__wp;
  const sim = wp.sim;
  const orig = sim.step;
  let simMs = 0;
  let frames = 0;
  let worst = 0;
  sim.step = (dt) => {
    const t0 = performance.now();
    const a = orig(dt);
    const d = performance.now() - t0;
    simMs += d;
    worst = Math.max(worst, d);
    frames++;
    return a;
  };
  const tick0 = sim.worldInfo()[0];
  const t0 = performance.now();
  await new Promise((r) => setTimeout(r, ms));
  const wall = (performance.now() - t0) / 1000;
  const ticks = sim.worldInfo()[0] - tick0;
  sim.step = orig;
  const speed = sim.states()[14];
  return {
    frames,
    fps: frames / wall,
    simMsPerFrame: simMs / frames,
    simMsPer60HzFrame: (simMs / ticks) * 4,
    worstFrameMs: worst,
    realTimeFactor: ticks / 240 / wall,
    vehicles: sim.vehicleMeta().length / 3,
    playerSpeedKmh: speed * 3.6,
    hudPerf: wp.app.perf,
  };
}, Number(secs) * 1000);
console.log(JSON.stringify({ car: car || "(default)", mode, ...result, errors: errors.slice(0, 3) }));
await browser.close();
process.exit(0);
