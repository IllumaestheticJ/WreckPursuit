// Art-style screenshots and perf for docs/media/art/ (machine S, render/style.ts).
// Usage (after `npm run wasm`): node e2e/art_shots.mjs [outDir]   Env: CHROMIUM, VIEWPORT=WxH.
// Shots: the city chase in Port Vesper at dawn / day / dusk / night in low-poly dark, the same
// night in greybox, the rainy night, a police close-up and a street-level view. Then a perf
// A/B in the night chase: frame / render ms in each style and the post pass alone (render with
// and without the post stack, GPU synced with readPixels; swiftshader = relative numbers).
// Prints one PERF line per measurement; exits 1 on page errors.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const out = resolve(process.argv[2] ?? join(root, "../../docs/media/art"));
mkdirSync(out, { recursive: true });
const PORT = 5196;
const URL = `http://localhost:${PORT}/`;

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; i < 100; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
  await new Promise((r) => setTimeout(r, 200));
}

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const [vw, vh] = (process.env.VIEWPORT ?? "1600x900").split("x").map(Number);
const page = await (await browser.newContext({ viewport: { width: vw, height: vh } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
await page.goto(`${URL}?style=lowpoly`);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });

/** City chase in Port Vesper at `hour` (the preset is 23:00), optionally rainy. */
const chase = async (hour, rain = false) => {
  await page.evaluate(
    ([h, r]) => {
      const w = window.__wp;
      document.getElementById("career").checked = false;
      document.getElementById("city-map").value = "vesper_v2";
      w.startCityChase("debug_sedan", "Arcade", r);
      const json = JSON.parse(r ? w.sim.cityChaseRainJson("debug_sedan", "Arcade", "vesper_v2") : w.sim.cityChaseCityJson("debug_sedan", "Arcade", "vesper_v2"));
      w.sim.init(0, JSON.stringify({ ...json, time_of_day_h: h }), false);
    },
    [hour, rain],
  );
  await page.waitForFunction(() => (window.__wp.scene.cities.get("vesper_v2")?.children.length ?? 0) > 0, null, { timeout: 90000 });
};
const hideUi = () => page.addStyleTag({ content: "#overlay { display: none !important; }" });

await chase(23);
await hideUi();
// Debug arrows / CG marker off: the shots are about the look.
await page.evaluate(() => {
  for (const id of ["cgVelocity", "tireForces", "slipColour"]) window.__wp.toggleLayer(id);
});
const shots = [
  ["dawn", 6.6],
  ["day", 12],
  ["dusk", 19.1],
  ["night", 23],
];
for (const [name, hour] of shots) {
  await chase(hour);
  await page.evaluate(() => window.__wp.setCamera("chaseFar"));
  await page.keyboard.down("KeyW");
  await page.waitForTimeout(3500);
  await page.keyboard.up("KeyW");
  await page.screenshot({ path: join(out, `${name}_chase.png`) });
  console.log("wrote", `${name}_chase.png`);
}

// Night in greybox for comparison (same session, style switched).
await page.evaluate(() => window.__wp.setStyle("greybox"));
await page.waitForTimeout(800);
await page.screenshot({ path: join(out, "night_chase_greybox.png") });
console.log("wrote night_chase_greybox.png");
await page.evaluate(() => window.__wp.setStyle("lowpoly"));

// Police close-up: the nearest police car with its siren forced on in the view (a short
// headless chase rarely gets a pursuit going), followed by the chase camera.
const cop = await page.evaluate(() => {
  const w = window.__wp;
  const m = w.sim.vehicleMeta();
  const me = w.app.views.get(w.app.player).root.position;
  let best = 0;
  let bestD = Infinity;
  for (let o = 0; o < m.length; o += 3) {
    if (m[o + 1] !== 2) continue;
    const v = w.app.views.get(m[o]);
    const d = v ? v.root.position.distanceTo(me) : Infinity;
    if (d < bestD) [best, bestD] = [m[o], d];
  }
  if (best) {
    const v = w.app.views.get(best);
    const set = v.setMeta.bind(v);
    v.setMeta = (flags, t) => set(flags | 1 | 2, t); // siren + headlights
  }
  return best;
});
if (cop) {
  // Follow the police car with the chase camera (focus = that unit), three light-bar phases.
  await page.evaluate((id) => {
    const w = window.__wp;
    w.app.focus = id;
    w.setCamera("chase");
  }, cop);
  await page.waitForTimeout(1500);
  for (let i = 0; i < 3; i++) {
    await page.screenshot({ path: join(out, `night_police_${i}.png`) });
    await page.waitForTimeout(170);
  }
  await page.evaluate(() => {
    const w = window.__wp;
    w.app.focus = w.app.player;
  });
  console.log("wrote night_police_{0,1,2}.png (light bar phases)");
} else console.log("no siren within 40 s: police shot skipped");

// Street level downtown at night (windows, lamps, signals).
await page.evaluate(() => window.__wp.lookAt([-160, -45, 14], [120, -48, 8]));
await page.waitForTimeout(1200);
await page.screenshot({ path: join(out, "night_downtown_street.png") });
console.log("wrote night_downtown_street.png");
await page.evaluate(() => window.__wp.lookAt([-420, -330, 120], [60, 40, 0]));
await page.waitForTimeout(1200);
await page.screenshot({ path: join(out, "night_downtown_aerial.png") });
console.log("wrote night_downtown_aerial.png");

// Rainy night: wet road + rain over the style.
await chase(23, true);
await page.evaluate(() => window.__wp.setCamera("chaseFar"));
await page.keyboard.down("KeyW");
await page.waitForTimeout(3500);
await page.keyboard.up("KeyW");
await page.screenshot({ path: join(out, "rainy_night_chase.png") });
console.log("wrote rainy_night_chase.png");

// Perf A/B in the night chase.
await chase(23);
await page.evaluate(() => window.__wp.setCamera("chase"));
await page.keyboard.down("KeyW");
await page.waitForTimeout(3000);
const measure = (style) =>
  page.evaluate(async (s) => {
    const w = window.__wp;
    w.setStyle(s);
    await new Promise((r) => setTimeout(r, 1500));
    const p0 = { ...w.app.perf };
    const t0 = performance.now();
    let frames = 0;
    await new Promise((resolve) => {
      const tick = () => {
        frames++;
        if (performance.now() - t0 < 6000) requestAnimationFrame(tick);
        else resolve();
      };
      requestAnimationFrame(tick);
    });
    const wall = performance.now() - t0;
    return { style: s, fps: (frames * 1000) / wall, renderMs: w.app.perf.renderMs, frameMs: w.app.perf.frameMs, calls: w.scene.renderer.info.render.calls, before: p0.renderMs };
  }, style);
const grey = await measure("greybox");
const low = await measure("lowpoly");
console.log(`PERF style frame (swiftshader ${vw}×${vh}, night chase): greybox ${grey.fps.toFixed(1)} fps, render ${grey.renderMs.toFixed(2)} ms; low-poly dark ${low.fps.toFixed(1)} fps, render ${low.renderMs.toFixed(2)} ms (${low.calls} draw calls in the last pass)`);
// Post pass alone: same frame rendered plainly vs through the post stack (both GPU-synced).
const post = await page.evaluate(() => {
  const w = window.__wp;
  w.app.paused = true;
  const r = w.scene.renderer;
  const gl = r.getContext();
  const px = new Uint8Array(4);
  const time = (fn) => {
    fn();
    gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
    const t0 = performance.now();
    for (let i = 0; i < 15; i++) {
      fn();
      gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
    }
    return (performance.now() - t0) / 15;
  };
  const plain = time(() => r.render(w.scene.scene, w.rig.camera));
  const styled = time(() => w.scene.render(w.rig.camera));
  w.style.bloom = false;
  const noBloom = time(() => w.scene.render(w.rig.camera));
  w.style.bloom = true;
  w.app.paused = false;
  return { plain, styled, noBloom, w: gl.drawingBufferWidth, h: gl.drawingBufferHeight };
});
console.log(
  `PERF post pass (swiftshader ${post.w}×${post.h}): scene ${post.plain.toFixed(2)} ms plain → ${post.styled.toFixed(2)} ms through the post stack ` +
    `(+${(post.styled - post.plain).toFixed(2)} ms; tonemap only ${post.noBloom.toFixed(2)} ms)`,
);
const lamps = await page.evaluate(() => window.__wp.style.lampCount("vesper_v2"));
console.log(`street lamps in Port Vesper: ${lamps}`);

await browser.close();
server.kill("SIGTERM");
if (errors.length) {
  console.error("page errors:", errors.slice(0, 5));
  process.exit(1);
}
process.exit(0);
