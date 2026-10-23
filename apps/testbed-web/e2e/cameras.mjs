// Camera check + screenshots in the night run preset (headless Chromium, Playwright).
// Usage: npm run e2e:cameras  (needs `npm run wasm` first). Env: CHROMIUM=/path/to/chrome,
// SHOTS=dir (default ../../docs/media/cameras).
//
// Checks: C cycles through every angle, the near chase cam stays close and low at speed
// (no zooming out), V held looks back, and the chosen angle survives a reload.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5198;
const URL = `http://localhost:${PORT}/`;
const shots = process.env.SHOTS ?? join(root, "../../docs/media/cameras");
mkdirSync(shots, { recursive: true });

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
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const context = await browser.newContext({ viewport: { width: 1280, height: 720 } });
const page = await context.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
const ready = () => page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });

/** Camera relative to the focused car: horizontal distance, height above it, speed. */
const camPose = () =>
  page.evaluate(() => {
    const { app, rig } = window.__wp;
    const car = app.views.get(app.focus).root.position; // sim world (Z up)
    const cam = rig.camera.position.clone();
    // Three world → sim world: (x, y, z) → (x, -z, y).
    const sx = cam.x;
    const sy = -cam.z;
    const sz = cam.y;
    return {
      dist: Math.hypot(sx - car.x, sy - car.y),
      height: sz - car.z,
      speed: window.__wp.sim.states()[14],
      mode: rig.mode,
      fov: rig.camera.fov,
    };
  });

await page.goto(URL);
await ready();
// Fresh viewer: no stored camera.
await page.evaluate(() => localStorage.removeItem("wp.testbed.camera.v1"));
await page.reload();
await ready();
await page.evaluate(() => window.__wp.startNightRun("debug_sedan", "Arcade"));
// Hide the debug panels so the shots show the camera, not the HUD.
await page.addStyleTag({ content: "#overlay > :not(.left-col), .left-col > :not(.hud-main) { display: none !important; }" });
// Plain car body for the shots (debug overlays off; this browser context only).
await page.evaluate(() => ["lattice", "tireForces", "cgVelocity", "contacts"].forEach((id) => window.__wp.toggleLayer(id)));
await page.keyboard.down("KeyW");
await page.waitForTimeout(3500);

// Near chase at night-run speed: close and low, not zoomed out.
const samples = [];
for (let i = 0; i < 6; i++) {
  samples.push(await camPose());
  await page.waitForTimeout(250);
}
const maxDist = Math.max(...samples.map((s) => s.dist));
const hs = samples.map((s) => s.height);
check(
  samples[0].mode === "chase" && maxDist <= 7.6 && Math.min(...hs) >= 1.4 && Math.max(...hs) <= 2.6,
  `near chase at ${(samples.at(-1).speed * 3.6).toFixed(0)} km/h: ${maxDist.toFixed(2)} m back (max), ` +
    `${Math.min(...hs).toFixed(2)}–${Math.max(...hs).toFixed(2)} m up, fov ${samples.at(-1).fov.toFixed(0)}°`,
);

const modes = await page.evaluate(() => window.__wp.cameraModes);
const seen = [];
for (let i = 0; i < modes.length; i++) {
  const p = await camPose();
  seen.push(p.mode);
  // TV camera needs a few seconds to plant a roadside spot and cut.
  await page.waitForTimeout(p.mode === "cinematic" ? 2500 : 900);
  await page.screenshot({ path: join(shots, `${String(i + 1).padStart(2, "0")}_${p.mode}.png`) });
  if (p.mode === "chase") {
    await page.keyboard.down("KeyV");
    const back = await page
      .waitForFunction(() => document.querySelector(".hud-main")?.textContent?.includes("look back"), null, { timeout: 5000 })
      .then(() => "look back", () => "");
    await page.waitForTimeout(400);
    await page.screenshot({ path: join(shots, `09_lookback.png`) });
    await page.keyboard.up("KeyV");
    check(back.includes("look back"), "V held: look-back view (HUD shows it)");
  }
  await page.keyboard.press("KeyC");
}
const back = await camPose();
check(
  seen.join(",") === modes.join(",") && back.mode === modes[0],
  `C cycles all ${modes.length} angles and wraps: ${seen.join(" → ")}`,
);
await page.keyboard.up("KeyW");

// Persistence: pick far chase, reload, it is still far chase.
await page.keyboard.press("Digit2");
await page.reload();
await ready();
const after = await page.evaluate(() => window.__wp.rig.mode);
check(after === "chaseFar", `chosen angle survives a reload (${after})`);
await page.evaluate(() => localStorage.removeItem("wp.testbed.camera.v1"));

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
console.log(failures ? `${failures} check(s) failed` : "all camera checks passed");
process.exit(failures ? 1 : 0);
