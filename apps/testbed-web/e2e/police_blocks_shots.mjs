// Screenshots of full-width police roadblocks and spike strips (owner 2026-10-28: "roadblocks
// and spike strips completely block the road"): two roadblocks (a downtown street, Loop 9) with
// their one barricaded gap, two spike strips across wide roads ending under their unit's car,
// and a car smashing through a barricade (golden_roadblock_barricade).
// Usage: node e2e/police_blocks_shots.mjs (needs `npm run wasm` first).
// Env: CHROMIUM, SHOTS=dir (default docs/media/police/blocks).
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5213;
const URL = `http://localhost:${PORT}/?gfx=high`;
const shots = process.env.SHOTS ?? join(root, "..", "..", "docs", "media", "police", "blocks");
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
const untilTick = (t) => page.waitForFunction((t) => window.__wp.sim.worldInfo()[0] >= t, t, { timeout: 180000 });
const STRIDE = 11;
const records = async () => {
  const f = await page.evaluate(() => Array.from(window.__wp.sim.policeTools()));
  const out = [];
  for (let o = 0; o < f.length; o += STRIDE) out.push({ kind: f[o], x: f[o + 1], y: f[o + 2], z: f[o + 3], hx: f[o + 8] });
  return out;
};
const near = (recs, p, r) => recs.filter((t) => Math.hypot(t.x - p[0], t.y - p[1]) < r);

// Only the 3D view.
const hideUi = () => page.addStyleTag({ content: "* { visibility: hidden !important } canvas { visibility: visible !important }" });

await page.evaluate(() => window.__wp.runScenario("police/blocks_showcase.ron", 0));
await untilTick(240 * 2.5);
let recs = await records();
// Sites as in the scenario: [pos, travel dir].
const sites = [
  { name: "roadblock_downtown", pos: [50.0, -250.0], dir: [1.0, 0.0], block: true },
  { name: "roadblock_loop9", pos: [-292.2, 1366.6], dir: [-0.999, -0.045], block: true },
  { name: "spikes_downtown", pos: [200.0, -300.0], dir: [0.0, -1.0], block: false },
  { name: "spikes_loop9", pos: [805.0, -1041.8], dir: [0.999, 0.05], block: false },
];
await hideUi();
for (const s of sites) {
  const line = s.block ? [s.pos[0] + s.dir[0] * 4, s.pos[1] + s.dir[1] * 4] : s.pos;
  const mine = near(recs, line, 40);
  const z = mine.length ? mine.reduce((a, t) => a + t.z, 0) / mine.length : 0;
  if (s.block) {
    check(mine.filter((t) => t.kind === 0).length >= 4, `${s.name}: barriers across the road (${mine.filter((t) => t.kind === 0).length})`);
    check(mine.filter((t) => t.kind === 2).length === 1, `${s.name}: one barricade in the gap`);
  } else {
    check(mine.filter((t) => t.kind === 1).length >= 1, `${s.name}: spike strip on the road`);
  }
  // From the racer's side, raised: the whole width with the gap / the strip's end under its car.
  const right = [s.dir[1], -s.dir[0]];
  const back = s.block ? 16 : 13;
  const eye = [line[0] - s.dir[0] * back + right[0] * 2, line[1] - s.dir[1] * back + right[1] * 2, z + 6.5];
  await page.evaluate(([e, t]) => window.__wp.lookAt(e, t), [eye, [line[0], line[1], z]]);
  await page.waitForTimeout(1500);
  await page.screenshot({ path: join(shots, `${s.name}.png`) });
  // Close on the gap (roadblocks): the wooden sawhorse.
  if (s.block) {
    const bc = mine.find((t) => t.kind === 2);
    if (bc) {
      const e2 = [bc.x - s.dir[0] * 5 - right[0] * 1.5, bc.y - s.dir[1] * 5 - right[1] * 1.5, bc.z + 1.6];
      await page.evaluate(([e, t]) => window.__wp.lookAt(e, t), [e2, [bc.x, bc.y, bc.z]]);
      await page.waitForTimeout(1200);
      await page.screenshot({ path: join(shots, `${s.name}_barricade.png`) });
    }
  }
}

// A car smashing through the barricade (flat test world).
await page.evaluate(() => window.__wp.runScenario("golden/roadblock_barricade.ron", 0));
await untilTick(240 * 0.5);
recs = await records();
const bc = recs.find((t) => t.kind === 2);
check(!!bc, "barricade before the hit");
if (bc) await page.evaluate(([e, t]) => window.__wp.lookAt(e, t), [[bc.x + 7, bc.y - 9, 3.5], [bc.x, bc.y, 0.6]]);
await page.waitForTimeout(800);
await page.screenshot({ path: join(shots, "barricade_before_hit.png") });
await page.waitForFunction(() => Array.from(window.__wp.sim.policeTools()).some((v, i) => i % 11 === 0 && v === 3), null, { timeout: 120000 });
await page.evaluate(() => {
  for (let k = 0; k < 6; k++) window.__wp.sim.step(1 / 60);
});
await page.waitForTimeout(500);
await page.screenshot({ path: join(shots, "barricade_smashed.png") });
recs = await records();
check(recs.some((t) => t.kind === 3) && !recs.some((t) => t.kind === 2), "barricade broken into pieces");

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
server.kill("SIGTERM");
process.exit(failed ? 1 : 0);
