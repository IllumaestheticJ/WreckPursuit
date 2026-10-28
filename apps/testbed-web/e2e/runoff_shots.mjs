// Screenshots of the touge run-offs (owner 2026-10-28: "touge tracks need more run off so we can
// slow down after we finish"): for each pass and each end, a high view down onto the finish
// line, the run-off and its gravel arrester bed, and a low view from just before the line
// looking along the run-off. The camera spots are in `e2e/runoff_spots.json` (finish line,
// direction out of the road, far end of the bed; world metres) and the course is unchanged, so
// the same spots on an older build show the old 40 m to the wall.
// Usage: node e2e/runoff_shots.mjs (needs the wasm package first). Env: CHROMIUM, SHOTS=dir,
// TAG=after|before (file-name suffix).
import { spawn } from "node:child_process";
import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = Number(process.env.PORT ?? 5211);
const URL = `http://localhost:${PORT}/?gfx=high`;
const shots = process.env.SHOTS ?? join(root, "shots-runoff");
const tag = process.env.TAG ?? "after";
const spots = JSON.parse(readFileSync(join(here, "runoff_spots.json"), "utf8"));
mkdirSync(shots, { recursive: true });

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
const page = await (await browser.newContext({ viewport: { width: 1280, height: 760 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 90000 });
await page.evaluate(() => {
  const c = document.getElementById("career");
  if (c) c.checked = false;
});
const step = (n) =>
  page.evaluate((n) => {
    for (let k = 0; k < n; k++) window.__wp.sim.step(1 / 60);
  }, n);
const ids = [...new Set(spots.map((s) => s.id))];
let first = true;
for (const id of ids) {
  await page.click("#sc-touge");
  await page.waitForSelector('[data-testid="touge-picker"]', { timeout: 10000 });
  if (first) await page.click('[data-testid="touge-set-rep"] button >> text="10000"');
  first = false;
  await page.click(`[data-touge="${id}"] button`);
  await page.click('[data-testid="touge-direction"] button:has-text("Uphill")');
  await page.click('[data-testid="touge-daylight"] button:has-text("Day")');
  await page.click('[data-testid="touge-picker"] button:has-text("Battle")');
  await page.waitForFunction((id) => window.__wp.sim.scenario().name.startsWith(`race_touge:${id}:`), id, { timeout: 60000 });
  await step(60);
  await page.waitForTimeout(1500);
  const css = await page.addStyleTag({ content: "* { visibility: hidden !important } canvas { visibility: visible !important }" });
  for (const s of spots.filter((s) => s.id === id)) {
    const [fx, fy, fz] = s.finish;
    const [dx, dy] = s.dir;
    const [bx, by, bz] = s.far;
    // High: above the middle of line → bed, a little back, looking down onto it.
    const mx = 0.5 * (fx + bx);
    const my = 0.5 * (fy + by);
    const mz = 0.5 * (fz + bz);
    await page.evaluate(([e, t]) => window.__wp.lookAt(e, t), [
      [mx - dx * 160, my - dy * 160, mz + 230],
      [mx + dx * 10, my + dy * 10, mz],
    ]);
    await step(1);
    await page.waitForTimeout(800);
    await page.screenshot({ path: join(shots, `${id}_${s.end}_high_${tag}.png`) });
    // Low: 20 m before the line, 4 m up, looking along the run-off.
    await page.evaluate(([e, t]) => window.__wp.lookAt(e, t), [
      [fx - dx * 20, fy - dy * 20, fz + 4],
      [fx + dx * 150, fy + dy * 150, fz],
    ]);
    await step(1);
    await page.waitForTimeout(800);
    await page.screenshot({ path: join(shots, `${id}_${s.end}_low_${tag}.png`) });
  }
  await css.evaluate((n) => n.remove());
  await page.evaluate(() => window.__wp.rig.setMode("chase"));
}
console.log(`run-off shots (${tag}) in ${shots}; errors: ${errors.slice(0, 3).join(" | ") || "none"}`);
await browser.close();
process.exit(0);
