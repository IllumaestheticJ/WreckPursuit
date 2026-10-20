// Drift lab (owner rows 90 / 91 / 98): ?drift=1 opens a flat-world free drive at noon with the
// painted course and the tuning panel; a slider writes the runtime tuning through wp_wasm and
// is remembered in wp.driftTuning.v4; the readout shows slide angle and assist mode; F8 hides the
// panel. Saves a screenshot to <tmpdir>/drift_lab.png. `node e2e/drift_lab.mjs`
import { spawn } from "node:child_process";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 5198;
const URL = `http://localhost:${PORT}/?drift=1`;
const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; i < 100; i++) {
  try {
    if ((await fetch(`http://localhost:${PORT}/`)).ok) break;
  } catch {}
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
const page = await (await browser.newContext({ viewport: { width: 1400, height: 860 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
await page.waitForTimeout(500);
const tuning = () => page.evaluate(() => window.__wp.sim.driftTuning(window.__wp.app.player));
check(await page.locator("#drift-panel").isVisible(), "tuning panel open");
const info = await page.evaluate(() => ({ city: window.__wp.sim.cityId(), n: window.__wp.sim.entities().length }));
check(info.city === "" && info.n === 1, `flat world, player only (${JSON.stringify(info)})`);
const t0 = await tuning();
check(t0.length === 11 && Math.abs(t0[9] - 7) < 1e-6 && Math.abs(t0[0] - 0.03) < 1e-6 && t0[6] === 0 && t0[8] === 0, `defaults from the sim, smoothing and hold off (${t0.map((v) => v.toFixed(3))})`);
// Third slider = slew boost (index 2 of the tuning array).
const slew = page.locator("#drift-panel input[type=range]").nth(3);
await slew.fill("6");
await slew.dispatchEvent("input");
check(Math.abs((await tuning())[2] - 6) < 1e-6, "slew boost slider → sim tuning");
const saved = await page.evaluate(() => JSON.parse(localStorage.getItem("wp.driftTuning.v4") ?? "[]"));
check(saved.length === 12 && saved[3] === 6, `saved in wp.driftTuning.v4 (${JSON.stringify(saved)})`);
const cst = page.locator("#drift-panel input[type=range]").nth(0);
await cst.fill("80");
await cst.dispatchEvent("input");
const help = await page.evaluate(() => window.__wp.sim.assists(window.__wp.app.player).countersteer_help);
check(Math.abs(help - 0.8) < 1e-6, `countersteer slider → assists (${help})`);
// Drive: throttle + steer for a moment, the readout updates.
await page.keyboard.down("KeyW");
await page.keyboard.down("KeyD");
await page.waitForTimeout(1500);
await page.keyboard.up("KeyD");
await page.keyboard.up("KeyW");
const text = await page.textContent("#drift-panel");
check(/slide [0-9.]+° · mode \w+ · envelope/.test(text), `readout live (${text.match(/slide[^·]+·[^·]+·[^d]+/)?.[0]})`);
await page.screenshot({ path: join(tmpdir(), "drift_lab.png") });
await page.keyboard.press("F8");
check(!(await page.locator("#drift-panel").isVisible()), "F8 hides the panel");
// A reload keeps the tuning.
await page.reload();
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
await page.waitForTimeout(300);
check(Math.abs((await tuning())[2] - 6) < 1e-6, "tuning kept after a reload");
check(errors.length === 0, `no page errors ${errors.join(" | ")}`);
await browser.close();
console.log(failures ? `${failures} drift lab check(s) failed` : "all drift lab checks passed");
process.exit(failures ? 1 : 0);
