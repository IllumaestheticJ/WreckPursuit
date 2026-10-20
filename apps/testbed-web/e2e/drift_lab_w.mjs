// Drift Lab W (lead/drift-lab-w): ?drift=w opens the lab with the parity car, plain steering and
// the lab's ride tuning; the tune panel changes the overrides; a held key moves the rack at the
// plain rate; every parity car starts (car, panels, mesh) from the car selector.
// `npm run build && node e2e/drift_lab_w.mjs` (PREVIEW=0: against `vite` dev instead of the
// built site; SHOT=path.png saves a screenshot).
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 5198;
const URL = `http://localhost:${PORT}/?drift=w`;
const preview = process.env.PREVIEW !== "0";
const server = spawn("npx", preview ? ["vite", "preview", "--port", String(PORT), "--strictPort"] : ["vite", "--port", String(PORT), "--strictPort"], {
  cwd: root,
  stdio: "pipe",
});
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; i < 150; i++) {
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
page.on("console", (m) => {
  if (m.type() === "error" && /failed to start|not in data bundle/.test(m.text())) errors.push(m.text());
});
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 90000 });
const sim = () => page.evaluate(() => window.__wp.sim.scenario());
let sc = await sim();
check(sc.name === "drift_lab_w", `scenario ${sc.name}`);
check(sc.entities?.[0]?.car === "parity_rwd_d", `default car ${sc.entities?.[0]?.car}`);
check((await page.title()) === "Drift Lab W", `title "${await page.title()}"`);
check(!sc.traffic_cars && !sc.police_cars, "no traffic, no police");
const plain = await page.evaluate(() => window.__wp.sim.plainSteer());
check(plain && Math.abs(plain.t_full_s - 0.28) < 1e-6 && Math.abs(plain.t_return_s - 0.33) < 1e-6, `plain steering on ${JSON.stringify(plain)}`);
const ride = await page.evaluate(() => window.__wp.sim.rideTune());
check(ride && Math.abs(ride.arb - 0.25) < 1e-6, `ride lab default ${JSON.stringify(ride)}`);
const a = await page.evaluate(() => window.__wp.sim.assists(window.__wp.app?.player ?? window.__wp.sim.entities()[0]));
check(!a.tcs && !a.esc && !a.auto_gears, `assists TCS ${a.tcs} ESC ${a.esc} auto gears ${a.auto_gears}`);
check(await page.locator("#drift-w-tune").isVisible(), "tune button shown");
const sliders = await page.locator("#drift-lab-w input[type=range]").count();
check(sliders >= 18, `${sliders} sliders (steering + suspension + car / tyre)`);
check((await page.evaluate(() => window.__wp.sim.carTune()?.lat_under_spin)) === 1, "car / tyre experiments start at the data");
await page.locator("#drift-lab-w button", { hasText: "spin grip + damper" }).click();
const cand = await page.evaluate(() => window.__wp.sim.carTune());
check(Math.abs(cand.lat_under_spin - 0.15) < 1e-6 && Math.abs(cand.yaw_damping - 0.2) < 1e-6, `spin grip + damper ${JSON.stringify(cand)}`);
await page.locator("#drift-lab-w button", { hasText: "tyre data" }).click();
check((await page.evaluate(() => window.__wp.sim.carTune().yaw_damping)) === 0, "car data restores");
await page.locator("#drift-lab-w button", { hasText: "spin model" }).click();
const spin = await page.evaluate(() => window.__wp.sim.carTune());
check(spin.spin_model === true && spin.yaw_damping === 0 && Math.abs(spin.spin_fx_slide - 0.958) < 1e-4, `spin model preset ${JSON.stringify(spin)}`);
await page.locator("#drift-lab-w button", { hasText: "spin + drift lock" }).click();
const sl = await page.evaluate(() => window.__wp.sim.carTune());
check(sl.spin_model === true && Math.abs(sl.drift_lock_extra + 0.5) < 1e-4 && Math.abs(sl.front_deep_falloff - 0.13) < 1e-4, `spin + drift lock preset ${JSON.stringify(sl)}`);
await page.locator("#drift-lab-w button", { hasText: "tyre data" }).click();
check((await page.evaluate(() => window.__wp.sim.carTune().spin_model)) === false, "tyre data turns the spin model off");
// The spin model checkbox alone (default off) turns it on and off.
const spinBox = page.locator("#drift-lab-w label", { hasText: "spin model (tyre-slip)" }).locator("input[type=checkbox]");
check(!(await spinBox.isChecked()), "spin model checkbox off by default");
await spinBox.check();
check((await page.evaluate(() => window.__wp.sim.carTune().spin_model)) === true, "spin model checkbox → on");
await spinBox.uncheck();
check((await page.evaluate(() => window.__wp.sim.carTune().spin_model)) === false, "spin model checkbox → off");
// Steering slider → override.
await page.evaluate(() => {
  const inputs = [...document.querySelectorAll("#drift-lab-w input[type=range]")];
  const s = inputs.find((i) => i.parentElement.textContent.includes("full lock time"));
  s.value = "0.4";
  s.dispatchEvent(new Event("input"));
});
check(Math.abs((await page.evaluate(() => window.__wp.sim.plainSteer().t_full_s)) - 0.4) < 1e-6, "full lock time slider sets the override");
await page.locator("#drift-lab-w button", { hasText: "defaults" }).click();
check(Math.abs((await page.evaluate(() => window.__wp.sim.plainSteer().t_full_s)) - 0.28) < 1e-6, "defaults restore 0.28 s");
// Speed sensitivity A/B: tyre = full lock.
await page.selectOption("#drift-lab-w-sens", "tyre");
check((await page.evaluate(() => window.__wp.sim.plainSteer().sens_applies)) === "tyre", "sensitivity on: tyre");
await page.selectOption("#drift-lab-w-sens", "rack");
check((await page.evaluate(() => window.__wp.sim.plainSteer().sens_applies)) === "rack", "sensitivity on: rack");
// Hold the right key: the rack goes right and reaches full (raw key, constant-rate slew).
await page.keyboard.down("KeyD");
await page.waitForTimeout(700);
const held = await page.evaluate(() => Array.from(window.__wp.sim.plainReadout(window.__wp.sim.entities()[0])));
await page.keyboard.up("KeyD");
check(held[0] === 1 && held[1] > 0.95, `key right → rack ${held[1]?.toFixed(2)} (key ${held[0]})`);
const text = await page.locator("#drift-lab-w").innerText();
check(/Drift Lab W · plain steering/.test(text) && /rack/.test(text) && /roll .* Hz/.test(text), `readout: ${text.split("\n").slice(0, 5).join(" | ")}`);
if (process.env.SHOT) await page.screenshot({ path: process.env.SHOT });
// Every parity car starts from the selector.
for (const car of ["parity_rwd_a", "parity_rwd_b", "parity_fwd_c", "shiro_turbo", "parity_rwd_d"]) {
  await page.selectOption("#drift-lab-w select", car);
  await page.waitForFunction((c) => window.__wp.sim.scenario().entities?.[0]?.car === c, car, { timeout: 30000 }).catch(() => {});
  sc = await sim();
  check(sc.name === "drift_lab_w" && sc.entities?.[0]?.car === car, `selector → ${car}`);
}
check(errors.length === 0, `no page errors ${errors.join(" | ")}`);
await browser.close();
server.kill("SIGTERM");
process.exit(failures ? 1 : 0);
