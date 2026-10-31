// Chase-camera screenshots of arcade-classic-style roadblocks and a spike-strip deployment
// (roadblocks-v2, owner 2026-10-31): a wanted-level-1, -3 and -5 roadblock (police cars parked
// at an angle across a downtown street, gaps between them, spiked at higher heat, heavy SUVs
// and a second line at L5) and a spike strip with its unit beside it, each approached by the
// player's car (scenarios police/roadblock_tier{1,3,5}.ron, police/spike_deploy.ron). Checks
// the cones and flares are drawn, the block's cars are real vehicles and the HUD shows its
// "Roadblock ahead" / "Spike strip ahead" alert.
// Usage: node e2e/police_blocks_shots.mjs (needs `npm run wasm` first).
// Env: CHROMIUM, SHOTS=dir (default docs/media/roadblocks-v2).
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5213;
const URL = `http://localhost:${PORT}/?gfx=high`;
const shots = process.env.SHOTS ?? join(root, "..", "..", "docs", "media", "roadblocks-v2");
mkdirSync(shots, { recursive: true });

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; i < 100; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
  await new Promise((r) => setTimeout(r, 200));
}

let failed = 0;
const check = (ok, what) => {
  console.log(`${ok ? "PASS" : "FAIL"} ${what}`);
  if (!ok) failed++;
};

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1280, height: 760 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 90000 });
const untilTick = (t) => page.waitForFunction((t) => window.__wp.sim.worldInfo()[0] >= t, t, { timeout: 240000 });
const STRIDE = 11;
const kinds = async () => {
  const f = await page.evaluate(() => Array.from(window.__wp.sim.policeTools()));
  const k = [];
  for (let o = 0; o < f.length; o += STRIDE) k.push(f[o]);
  return k;
};
const count = (k, kind) => k.filter((x) => x === kind).length;

const runs = [
  { name: "roadblock_l1", file: "police/roadblock_tier1.ron", spikes: 0, alert: "ROADBLOCK AHEAD" },
  { name: "roadblock_l3", file: "police/roadblock_tier3.ron", spikes: 1, alert: "ROADBLOCK AHEAD" },
  { name: "roadblock_l5", file: "police/roadblock_tier5.ron", spikes: 2, alert: "ROADBLOCK AHEAD" },
  { name: "spike_strip", file: "police/spike_deploy.ron", spikes: 1, alert: "SPIKE STRIP AHEAD" },
];
for (const r of runs) {
  await page.evaluate((f) => window.__wp.runScenario(f, 0), r.file);
  await page.evaluate(() => window.__wp.setCamera("chase"));
  // Mid-approach: the alert is up and the block is in view down the street.
  await untilTick(240 * 4.5);
  const alert = await page.evaluate(() => {
    const el = document.getElementById("tool-alert");
    return el && !el.classList.contains("hidden") ? el.textContent ?? "" : "";
  });
  check(alert.includes(r.alert), `${r.name}: HUD alert "${alert}"`);
  await page.waitForTimeout(800);
  await page.screenshot({ path: join(shots, `${r.name}_approach.png`) });
  // Stopped short of it.
  await untilTick(240 * 9.0);
  await page.waitForTimeout(800);
  await page.screenshot({ path: join(shots, `${r.name}.png`) });
  const k = await kinds();
  check(count(k, 1) >= r.spikes, `${r.name}: ${count(k, 1)} spike strip(s) drawn (want ${r.spikes}+)`);
  check(count(k, 5) >= 2, `${r.name}: road flares drawn (${count(k, 5)})`);
  if (r.name.startsWith("roadblock")) check(count(k, 4) >= 4, `${r.name}: cones drawn (${count(k, 4)})`);
  check(count(k, 0) + count(k, 2) + count(k, 3) === 0, `${r.name}: no retired barrier / barricade records`);
}

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
server.kill("SIGTERM");
process.exit(failed ? 1 : 0);
