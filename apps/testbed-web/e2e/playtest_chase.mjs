// Night-run visual playtest in headless Chromium: drives the chase, screenshots it, reports
// overlapping HUD panels, checks every layer key toggles its layer (and that camera keys 1–8
// pick cameras), and screenshots the F1 help.
// Usage: node e2e/playtest_chase.mjs [outDir] [viewport WxH]   (needs `npm run wasm`).
// Prints one PASS/FAIL line per check and exits non-zero on failure.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 5197;
const URL = `http://localhost:${PORT}/`;
const [out = join(tmpdir(), "wp-playtest"), size = "1400x860"] = process.argv.slice(2);
const [W, H] = size.split("x").map(Number);
mkdirSync(out, { recursive: true });

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
const page = await (await browser.newContext({ viewport: { width: W, height: H } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
const shot = (name) => page.screenshot({ path: join(out, name) });

await page.evaluate(() => window.__wp.startNightRun("debug_sedan", "Arcade"));
await page.keyboard.down("KeyW");
// 12 s of sim time (SwiftShader runs the city below real time).
await page.waitForFunction(() => window.__wp.sim.worldInfo()[0] > 240 * 12, null, { timeout: 180000 });
await page.keyboard.up("KeyW");
await shot("chase_hud.png");

// Overlaps between visible HUD boxes (help excluded: it is a modal on top).
const overlaps = await page.evaluate(() => {
  const boxes = [];
  const add = (name, el) => {
    if (!el) return;
    const s = getComputedStyle(el);
    if (s.display === "none" || s.visibility === "hidden" || el.closest(".hidden")) return;
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) return;
    boxes.push({ name, r: { x: r.left, y: r.top, w: r.width, h: r.height } });
  };
  add("controls", document.getElementById("controls"));
  for (const c of ["hud-main", "inputs", "silhouette", "comps", "regions", "feed", "telemetry"]) add(c, document.querySelector(`.${c}`));
  // UI-kit chase readout: the layer and its root cover the page, so take the outermost
  // descendants that are panel-sized (smaller than 60 % of the viewport width).
  document.querySelectorAll("#ui-chase *").forEach((el, i) => {
    const small = (e) => e.getBoundingClientRect().width < innerWidth * 0.6;
    if (small(el) && el.parentElement && !small(el.parentElement)) add(`chase:${String(el.className || el.tagName).split(" ")[0]}#${i}`, el);
  });
  document.querySelectorAll("#overlay > *").forEach((el) => {
    if (!["controls", "error"].includes(el.id) && !el.matches(".left-col,.right-col,.help,.telemetry,#ui-chase,#ui-bill,#pursuit-banner"))
      add(`other:${el.id || String(el.className).split(" ")[0]}`, el);
  });
  add("banner", document.getElementById("pursuit-banner"));
  const hits = [];
  for (let i = 0; i < boxes.length; i++)
    for (let j = i + 1; j < boxes.length; j++) {
      const a = boxes[i].r;
      const b = boxes[j].r;
      const ox = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
      const oy = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
      // Nested chase-kit boxes overlap their own parent by design.
      if (boxes[i].name.startsWith("chase:") && boxes[j].name.startsWith("chase:")) continue;
      if (ox > 2 && oy > 2) hits.push(`${boxes[i].name} × ${boxes[j].name} (${ox.toFixed(0)}×${oy.toFixed(0)} px)`);
    }
  return { hits, boxes: boxes.map((b) => `${b.name} ${b.r.x.toFixed(0)},${b.r.y.toFixed(0)} ${b.r.w.toFixed(0)}×${b.r.h.toFixed(0)}`) };
});
console.log("boxes:", overlaps.boxes.join(" | "));
check(overlaps.hits.length === 0, `no overlapping HUD panels at ${W}×${H}${overlaps.hits.length ? ": " + overlaps.hits.join("; ") : ""}`);

// Every layer key flips exactly its own layer.
const layerKeys = await page.evaluate(async () => {
  const { LAYERS } = await import("/src/settings.ts");
  return LAYERS.map((l) => ({ id: l.id, key: l.key }));
});
const bad = [];
for (const l of layerKeys) {
  const before = await page.evaluate(async (id) => (await import("/src/settings.ts")).layers.on(id), l.id);
  await page.keyboard.press(l.key);
  const after = await page.evaluate(async (id) => (await import("/src/settings.ts")).layers.on(id), l.id);
  await page.keyboard.press(l.key);
  const back = await page.evaluate(async (id) => (await import("/src/settings.ts")).layers.on(id), l.id);
  if (after === before || back !== before) bad.push(`${l.key}→${l.id}`);
}
check(bad.length === 0, `all ${layerKeys.length} layer keys toggle their layer${bad.length ? " (broken: " + bad.join(", ") + ")" : ""}`);
const keyClash = layerKeys.filter((l) => /^Digit[1-8]$/.test(l.key)).map((l) => l.key);
check(keyClash.length === 0, `no layer key on camera keys 1–8${keyClash.length ? " (clash: " + keyClash.join(", ") + ")" : ""}`);
const cams = [];
for (let n = 1; n <= 8; n++) {
  await page.keyboard.press(`Digit${n}`);
  cams.push(await page.evaluate(() => window.__wp.rig?.mode ?? window.__wp.cameraMode?.() ?? null));
}
check(new Set(cams).size === 8, `camera keys 1–8 pick 8 cameras: ${cams.join(", ")}`);
await page.keyboard.press("Digit1");

await page.keyboard.press("F1");
await shot("chase_help.png");
const help = await page.evaluate(() => {
  const el = document.querySelector(".help");
  const r = el.getBoundingClientRect();
  return { text: el.textContent ?? "", fits: r.top >= 0 && r.bottom <= innerHeight && r.left >= 0 && r.right <= innerWidth };
});
await page.keyboard.press("F1");
check(help.fits, "F1 help fits on screen");

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
console.log(failures ? `${failures} check(s) failed` : "all checks passed");
process.exit(failures ? 1 : 0);
