// Drift Lab C (lead/drift-lab-c, built on B): ?drift=c opens the lab (no traffic / police), the
// tune panel sets the runtime override, the readout follows the player. `node e2e/drift_lab_b.mjs`
// (SHOT=path.png saves a screenshot.)
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 5199;
const URL = `http://localhost:${PORT}/?drift=${process.env.DRIFT ?? "c"}&car=shiro_turbo`;
const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; i < 100; i++) {
  try {
    if ((await fetch(URL)).ok) break;
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
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
const sc = await page.evaluate(() => window.__wp.sim.scenario());
check(sc.name === "drift_lab_c", `scenario ${sc.name}`);
check(!sc.traffic_cars && !sc.police_cars, "no traffic, no police");
check(await page.locator("#drift-tune").isVisible(), "tune button shown");
await page.locator("#drift-tune").click();
const sliders = await page.locator("#drift-lab-b input[type=range]").count();
check(sliders >= 5, `${sliders} sliders`);
// Move the slew slider: the override follows.
const before = await page.evaluate(() => window.__wp.sim.driftTuning().catch_lock_s);
await page.evaluate(() => {
  const inputs = [...document.querySelectorAll("#drift-lab-b input[type=range]")];
  const slew = inputs.find((i) => i.parentElement.textContent.includes("slew"));
  slew.value = "0.2";
  slew.dispatchEvent(new Event("input"));
});
const after = await page.evaluate(() => window.__wp.sim.driftTuning().catch_lock_s);
check(Math.abs(after - 0.2) < 1e-6 && before !== after, `slew override ${before} -> ${after}`);
// Drive a little: the readout fills in.
await page.keyboard.down("KeyW");
await page.waitForTimeout(2500);
await page.keyboard.up("KeyW");
const text = await page.locator("#drift-lab-b").innerText();
check(/Drift Lab C/.test(text) && /slide .*° · held/.test(text), `readout: ${text.split("\n").slice(1, 4).join(" | ")}`);
check(/throttle key \d+% → engine \d+%/.test(text), "readout shows key → engine throttle");
check((await page.evaluate(() => window.__wp.sim.driftTuning().throttle_shape)) === 0, "throttle shaping off by default");
if (process.env.SHOT) await page.screenshot({ path: process.env.SHOT });
await page.locator("#drift-lab-b button", { hasText: "defaults" }).click();
check((await page.evaluate(() => window.__wp.sim.driftTuning().catch_lock_s)) === before, "defaults restore");
check(errors.length === 0, `no page errors ${errors.join(" | ")}`);
await browser.close();
server.kill("SIGTERM");
process.exit(failures ? 1 : 0);
