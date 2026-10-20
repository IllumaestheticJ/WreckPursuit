// Art-style screenshots and perf for docs/media/art/ (machine S, render/style.ts).
// Usage (after `npm run wasm`): node e2e/art_shots.mjs [outDir]   Env: CHROMIUM, VIEWPORT=WxH.
// Shots: the city chase in Port Vesper at dawn / day / dusk / night in low-poly dark, the same
// night in greybox, the rainy night, a police close-up and street-level / aerial views downtown.
// Perf A/B: e2e/art_perf.mjs. Exits 1 on page errors.
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
// gfx=high: the full look (native resolution, MSAA, light pools, bloom) as on a hardware GPU;
// headless Chromium is a software rasterizer, where "auto" picks the low profile.
await page.goto(`${URL}?style=lowpoly&gfx=${process.env.GFX ?? "high"}`);
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
    // Siren + headlights; `__copT` pins the strobe phase (screenshots at ~1 fps would otherwise
    // sample it at random).
    v.setMeta = (flags, t) => set(flags | 1 | 2, window.__copT ?? t);
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
  // Phases of the 2.5 Hz pattern (vehicle_view.ts setMeta): red strobe, blue strobe, gap.
  const phases = [0.05, 0.55, 0.16];
  for (let i = 0; i < 3; i++) {
    await page.evaluate(async (t) => {
      window.__copT = t / 2.5;
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    }, phases[i]);
    await page.screenshot({ path: join(out, `night_police_${i}.png`) });
  }
  await page.evaluate(() => delete window.__copT);
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

const lamps = await page.evaluate(() => window.__wp.style.lampCount("vesper_v2"));
console.log(`street lamps in Port Vesper: ${lamps}`);

await browser.close();
server.kill("SIGTERM");
if (errors.length) {
  console.error("page errors:", errors.slice(0, 5));
  process.exit(1);
}
process.exit(0);
