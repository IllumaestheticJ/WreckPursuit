// Indicator close-ups for docs/media/indicators/ (playtest row 136, "turn signals look plopped
// on"): five body styles, front and rear three-quarter views, indicators lit (hazards: both
// sides) and unlit, from a fixed camera relative to the car. Only uses hooks `main` already
// had (startFreeDrive, lookFrom, VehicleView.setIndicators), so the same script makes the
// "before" and "after" sets.
//
// Usage (after `npm run wasm`): node e2e/indicator_shots.mjs <outDir> [testbedDir]
// e.g. node e2e/indicator_shots.mjs ../../docs/media/indicators/after
// Env: CHROMIUM=/path/to/chrome, CARS=a,b,c (default: the five below), HOUR (default 12).
//
// Output: <car>_<front|rear>_<lit|unlit>.png (640×360).
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { chromiumPath } from "./chromium.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const out = resolve(process.argv[2] ?? join(here, "../../../docs/media/indicators/after"));
const root = resolve(process.argv[3] ?? join(here, ".."));
mkdirSync(out, { recursive: true });
// Hatch, tuner, muscle, van (traffic), police.
const CARS = (process.env.CARS ?? "stoat_16v,arashi_t4,bulwark_400,satchel_20,sentinel_cruiser").split(",");
const HOUR = Number(process.env.HOUR ?? 12);
const PORT = 5196;
const URL = `http://localhost:${PORT}/`;

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; ; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
  if (i > 150) throw new Error("vite did not start");
  await new Promise((r) => setTimeout(r, 200));
}

const browser = await chromium.launch({
  executablePath: chromiumPath(),
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 640, height: 360 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 180000 });
await page.addStyleTag({ content: "#overlay { display: none !important; }" });
// Full resolution (no adaptive scale: software GL is slow) and no debug overlays on the car.
await page.evaluate(() => {
  const wp = window.__wp;
  const q = wp.scene?.quality;
  if (q) {
    q.profile = { ...q.profile, adaptive: false, maxPixelRatio: 1 };
    q.scale = 1;
    q.renderer?.setPixelRatio(1);
  }
  for (const id of ["tireForces", "slipColour", "contacts", "cgVelocity", "suspension", "lattice", "stress"]) if (wp.layersOn(id)) wp.toggleLayer(id);
});
const frames = (n = 3) => page.evaluate((n) => new Promise((r) => { const f = (k) => (k ? requestAnimationFrame(() => f(k - 1)) : r()); f(n); }), n);

for (const car of CARS) {
  // Test pad free drive (no city: nothing else in frame), parked; let the suspension settle,
  // then freeze the sim and force the indicator mask (the per-frame stalk call is overridden).
  await page.evaluate(([car, hour]) => {
    const wp = window.__wp;
    wp.app.paused = false;
    wp.startFreeDrive(car, "Arcade", false, hour);
  }, [car, HOUR]);
  await page.waitForFunction(() => window.__wp.app.views.get(window.__wp.app.player)?.panels.ready === true, null, { timeout: 60000 });
  await page.waitForTimeout(1500);
  const pose = await page.evaluate(() => {
    const wp = window.__wp;
    wp.app.paused = true;
    // The world-origin axes gizmo sits right by the spawn point.
    wp.scene.scene.traverse((o) => o.type === "AxesHelper" && (o.visible = false));
    const v = wp.app.views.get(wp.app.player);
    window.__indMask = 0;
    const set = v.setIndicators.bind(v);
    v.setIndicators = () => set(window.__indMask);
    const p = v.root.position;
    const q = v.root.quaternion;
    // Chassis +Y in world.
    const fx = 2 * (q.x * q.y - q.w * q.z);
    const fy = 1 - 2 * (q.x * q.x + q.z * q.z);
    const n = Math.hypot(fx, fy) || 1;
    const [w, l, h] = v.info.body_size_m;
    return { pos: [p.x, p.y, p.z], fwd: [fx / n, fy / n], w, l, h };
  });
  const { pos, fwd, l, h } = pose;
  const right = [fwd[1], -fwd[0]];
  for (const end of ["front", "rear"]) {
    // Three-quarter view from the car's left (x < 0), 1.7 m off the nose / tail, 1.6 m out.
    const s = end === "front" ? 1 : -1;
    const off = l / 2 + 1.7;
    const eye = [pos[0] + fwd[0] * s * off - right[0] * 1.6, pos[1] + fwd[1] * s * off - right[1] * 1.6, pos[2] + h * 0.45 + 0.25];
    const target = [pos[0] + fwd[0] * s * (l / 2 - 0.4), pos[1] + fwd[1] * s * (l / 2 - 0.4), pos[2] + h * 0.4];
    await page.evaluate(([e, t]) => window.__wp.lookFrom(e, t), [eye, target]);
    for (const lit of [true, false]) {
      await page.evaluate((m) => (window.__indMask = m), lit ? 3 : 0);
      await frames(4);
      const name = `${car}_${end}_${lit ? "lit" : "unlit"}.png`;
      await page.screenshot({ path: join(out, name), type: "png" });
      console.log("wrote", name);
    }
  }
}

await browser.close();
server.kill("SIGTERM");
if (errors.length) {
  console.error("page errors:", errors.slice(0, 3));
  process.exit(1);
}
process.exit(0);
