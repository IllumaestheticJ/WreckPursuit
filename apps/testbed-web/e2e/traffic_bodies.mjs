// Row 149 ("I currently now can't hit traffic cars"): free drive by day with the 16 street
// racers and the police uses up the vehicle slots; 0.12.49 promoted 16 traffic cars to physics
// with room for 10, and 6 had no collider (the player drove through them). Over ~40 s of
// driving, every traffic draw record flagged physical (flags & 1) must have a sim entity, and
// some traffic must be physical. `node e2e/traffic_bodies.mjs [chase]`.
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5208;
const URL = `http://localhost:${PORT}/`;
const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; i < 100; i++) { try { if ((await fetch(URL)).ok) break; } catch {} await new Promise((r) => setTimeout(r, 200)); }
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM, args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"] });
const page = await browser.newPage({ viewport: { width: 960, height: 540 } });
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.sim, null, { timeout: 120000 });
const chase = process.argv[2] === "chase";
await page.evaluate((chase) => (chase ? window.__wp.startCityChase() : window.__wp.startFreeDrive(undefined, undefined, true)), chase);
await page.keyboard.down("KeyW");
let worst = 0, physMax = 0;
for (let k = 0; k < 12; k++) {
  await page.waitForTimeout(5000);
  const r = await page.evaluate(() => {
    const sim = window.__wp.sim;
    const stride = sim.lodDrawStride();
    const rec = sim.trafficDraw();
    let phys = 0, orphan = 0, bodies = 0;
    for (let o = 0; o < rec.length; o += stride) {
      const physical = (rec[o + 9] & 1) === 1;
      if (physical) phys++;
      if (rec[o + 1] > 0) bodies++;
      if (physical && !(rec[o + 1] > 0)) orphan++;
    }
    return { t: (sim.worldInfo()[0] / 240).toFixed(0), vehicles: sim.vehicleMeta().length / 3, agents: rec.length / stride, phys, bodies, orphan, street: sim.streetRacers().length / 6 };
  });
  console.log(JSON.stringify(r));
  worst = Math.max(worst, r.orphan);
  physMax = Math.max(physMax, r.phys);
}
await page.keyboard.up("KeyW");
await browser.close();
const ok = worst === 0 && physMax > 0;
console.log(ok ? "PASS traffic bodies" : `FAIL: ${worst} physical traffic cars without a collider, ${physMax} physical at most`);
process.exit(ok ? 0 : 1);
