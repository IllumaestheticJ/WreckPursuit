// Brake dive check (owner playtest row 92, "headlights clip into the ground under braking"),
// headless Chromium (Playwright). Run alone: `npm run e2e:brake-dive` (needs `npm run wasm`
// first). Env: CHROMIUM=/path/to/chrome.
//
// Plays the scripted hard stop `vehicle/brake_dive_100_{arcade,sim}.ron` (three soft cars,
// full brake from 100 km/h on flat asphalt) and, every rendered frame, reads the vertices of
// the drawn panel meshes: the lowest body vertex ahead of the front axle must stay above the
// road (z = 0 on the flat world) and the drawn tyres must sit on it (wheel placement matches
// the physics' static compression). Also reports the peak nose-down pitch of the drawn body.
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { chromiumPath } from "./chromium.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5197;
const URL = `http://localhost:${PORT}/`;

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; ; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
  if (i > 100) throw new Error("vite did not start");
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
const context = await browser.newContext({ viewport: { width: 1200, height: 760 } });
const page = await context.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));

await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });

for (const file of ["vehicle/brake_dive_100_arcade.ron", "vehicle/brake_dive_100_sim.ron"]) {
  await page.evaluate((f) => window.__wp.runScenario(f), file);
  // Wait until every car's panel mesh is drawn.
  await page.waitForFunction(
    () => [...window.__wp.app.views.values()].every((v) => v.panels.ready),
    null,
    { timeout: 60000 },
  );
  // Per-frame probe of the drawn meshes (runs inside the page's own animation frames).
  await page.evaluate(() => {
    const wp = window.__wp;
    const out = new Map();
    window.__dive = out;
    const probe = () => {
      if (window.__dive !== out) return;
      for (const [id, view] of wp.app.views) {
        const root = view.root;
        root.updateMatrixWorld(true);
        // Mesh vertex → chassis space (inverse of the root's world matrix) → sim space (the
        // root's own pose = the interpolated sim pose; the scene above it may be offset).
        const inv = root.matrixWorld.clone().invert();
        const toSim = (p) => p.applyMatrix4(inv).applyMatrix4(root.matrix);
        const axleY = view.info.wheels[0].position_m[1];
        let r = out.get(id);
        if (!r) {
          r = { car: view.info.car_id, minFront: Infinity, minTyre: Infinity, maxTyre: -Infinity, minPitch: Infinity, pitch0: null, frames: 0 };
          out.set(id, r);
        }
        root.updateMatrix();
        const v = root.position.clone();
        let front = Infinity;
        view.panels.root.traverse((o) => {
          if (!o.isMesh || !o.visible) return;
          const pos = o.geometry.attributes.position;
          o.updateMatrixWorld(true);
          for (let i = 0; i < pos.count; i++) {
            v.fromBufferAttribute(pos, i).applyMatrix4(o.matrixWorld).applyMatrix4(inv);
            if (v.y > axleY) front = Math.min(front, v.applyMatrix4(root.matrix).z);
          }
        });
        // Lowest point of each drawn tyre (the wheel parts hang under the wheel pivots).
        let tyreLow = Infinity;
        for (const parts of view.panels.wheelParts) {
          let low = Infinity;
          for (const m of parts) {
            m.updateMatrixWorld(true);
            const pos = m.geometry.attributes.position;
            for (let i = 0; i < pos.count; i++) low = Math.min(low, toSim(v.fromBufferAttribute(pos, i).applyMatrix4(m.matrixWorld)).z);
          }
          if (low < Infinity) {
            r.maxTyre = Math.max(r.maxTyre, low);
            tyreLow = Math.min(tyreLow, low);
          }
        }
        r.minTyre = Math.min(r.minTyre, tyreLow);
        // Nose-up pitch of the drawn body, deg.
        const fwd = v.set(0, 1, 0).applyQuaternion(root.quaternion);
        const pitch = (Math.asin(Math.max(-1, Math.min(1, fwd.z))) * 180) / Math.PI;
        if (r.pitch0 === null) r.pitch0 = pitch;
        r.minPitch = Math.min(r.minPitch, pitch);
        r.minFront = Math.min(r.minFront, front);
        r.frames++;
      }
      requestAnimationFrame(probe);
    };
    requestAnimationFrame(probe);
  });
  await page.waitForFunction(() => window.__wp.sim.worldInfo()[0] > 7.2 * 240, null, { timeout: 120000, polling: 100 });
  const res = await page.evaluate(() => [...window.__dive.values()]);
  await page.evaluate(() => (window.__dive = null));
  for (const r of res) {
    const dive = r.pitch0 - r.minPitch;
    console.log(
      `  ${file} ${r.car}: ${r.frames} frames, lowest front body vertex ${(r.minFront * 100).toFixed(1)} cm, ` +
        `nose-down ${dive.toFixed(2)}° (from the first frame), tyre bottoms ${(r.minTyre * 100).toFixed(1)}…${(r.maxTyre * 100).toFixed(1)} cm`,
    );
    check(r.frames > 30, `${r.car}: probed while driving (${r.frames} frames)`);
    check(r.minFront > 0.02, `${r.car}: drawn front body stays above the road (${(r.minFront * 100).toFixed(1)} cm)`);
    check(dive < 2.8, `${r.car}: drawn nose dive ${dive.toFixed(2)}° (≤ ~2.5° + the at-speed pose)`);
    check(r.minTyre > -0.015 && r.maxTyre < 0.015, `${r.car}: drawn tyres sit on the road (${(r.minTyre * 100).toFixed(1)}…${(r.maxTyre * 100).toFixed(1)} cm)`);
  }
}

check(errors.length === 0, `no page errors (${errors.slice(0, 3).join(" | ")})`);
await browser.close();
server.kill("SIGTERM");
console.log(failures ? `${failures} check(s) failed` : "all brake-dive checks passed");
process.exit(failures ? 1 : 0);
