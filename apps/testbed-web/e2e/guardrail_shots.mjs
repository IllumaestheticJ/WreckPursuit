// Screenshots of the touge guard rails (owner 2026-10-26: "connected together properly and able
// to be broken off"): the rail flaring out to a runoff apron, a straight run on a grade, and a
// car put into the rail at 20° / 25 m/s with the sections it broke off.
// Usage: node e2e/guardrail_shots.mjs (needs `npm run wasm` first). Env: CHROMIUM, SHOTS=dir.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5209;
const URL = `http://localhost:${PORT}/?gfx=high`;
const shots = process.env.SHOTS ?? join(root, "shots-guardrail");
mkdirSync(shots, { recursive: true });

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
const page = await (await browser.newContext({ viewport: { width: 1280, height: 760 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
await page.evaluate(() => {
  const c = document.getElementById("career");
  if (c) c.checked = false;
});
await page.click("#sc-touge");
await page.waitForSelector('[data-testid="touge-picker"]', { timeout: 10000 });
await page.click('[data-testid="touge-set-rep"] button >> text="10000"');
await page.click('[data-testid="touge-direction"] button:has-text("Uphill")');
await page.click('[data-testid="touge-daylight"] button:has-text("Day")');
await page.click('[data-testid="touge-picker"] button:has-text("Battle")');
await page.waitForFunction(() => window.__wp.sim.scenario().name.startsWith("race_touge:"), null, { timeout: 30000 });
const [player] = await page.evaluate(() => window.__wp.sim.entities());
const step = (n) =>
  page.evaluate((n) => {
    for (let k = 0; k < n; k++) window.__wp.sim.step(1 / 60);
  }, n);
await step(60 * 5); // countdown
const settle = async () => {
  for (let i = 0; i < 20; i++) {
    await step(1);
    await page.waitForTimeout(60);
  }
};
const place = (x, y, yaw, v) => page.evaluate(([p, x, y, yaw, v]) => window.__wp.sim.debugPlace(p, x, y, yaw, v), [player, x, y, yaw, v]);

// Spots on touge_hollow_crown (base → summit), 30 m before a rail meets a runoff apron.
const spots = [
  { name: "flare_left", x: -239.3, y: 317.5, yaw: -2.95 },
  { name: "flare_right", x: 0.0, y: 175.8, yaw: 0.0 },
  { name: "flare_right2", x: -314.7, y: 526.2, yaw: 0.227 },
  { name: "grade_left", x: -308.2, y: 323.7, yaw: -2.967 },
  { name: "grade_left2", x: -657.3, y: 104.5, yaw: 2.827 },
];
// Only the 3D view in the shots.
await page.addStyleTag({ content: "* { visibility: hidden !important } canvas { visibility: visible !important }" });
for (const s of spots) {
  // 2.5 m towards the rail's side of the road (left of forward is (-cos, -sin)).
  const side = s.name.includes("left") ? 1 : -1;
  await place(s.x - Math.cos(s.yaw) * 2.5 * side, s.y - Math.sin(s.yaw) * 2.5 * side, s.yaw, 0);
  await settle();
  await page.screenshot({ path: join(shots, `${s.name}.png`) });
}

// Crash: 20° into the left rail at 25 m/s, 40 m before the first spot's flare.
const rails0 = await page.evaluate(() => Array.from(window.__wp.sim.propsStatic()));
const c = spots[0];
const fx = -Math.sin(c.yaw);
const fy = Math.cos(c.yaw);
await place(c.x - fx * 30, c.y - fy * 30, c.yaw + (20 * Math.PI) / 180, 25);
for (let i = 0; i < 4; i++) {
  await step(15);
  await page.waitForTimeout(80);
  await page.screenshot({ path: join(shots, `crash_${i}.png`) });
}
await step(60);
await settle();
await page.screenshot({ path: join(shots, "crash_after.png") });
const moved = await page.evaluate((r0) => {
  const st = window.__wp.sim.propsState?.() ?? null;
  return st ? st.length : null;
}, rails0);
console.log("props state len", moved, "errors", errors.slice(0, 3));
await browser.close();
process.exit(0);
