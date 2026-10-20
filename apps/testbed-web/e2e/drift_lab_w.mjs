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
check((await page.evaluate(() => window.__wp.sim.carTune()?.spin_model)) === true, "car / tyre experiments start at the spin model (W default)");
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
// Grip balance (wp_vehicle::grip_lab): starts at "new" (smart lock); the A/B button flips it.
check((await page.evaluate(() => window.__wp.sim.gripBalance()?.lock_mode)) === "smart", "grip balance starts at new (smart lock)");
const abText = async () => (await page.locator("#drift-w-balance-ab").innerText()).trim();
check((await abText()) === "balance: new", `A/B button reads "${await abText()}" (balance: new)`);
await page.locator("#drift-w-balance-ab").click();
check((await page.evaluate(() => window.__wp.sim.gripBalance()?.lock_mode)) === "plain", `A/B → before (plain lock, tyre data): ${JSON.stringify(await page.evaluate(() => window.__wp.sim.gripBalance()))}`);
check((await abText()) === "balance: before", `A/B button reads "${await abText()}" (balance: before)`);
await page.locator("#drift-w-balance-ab").click();
check((await page.evaluate(() => window.__wp.sim.gripBalance()?.lock_mode)) === "smart", "A/B → new again");
const gNew = await page.evaluate(() => window.__wp.sim.gripBalance());
check(Math.abs(gNew.rear_stiffness - 0.45) < 1e-6 && gNew.tyres_rwd_only === true && gNew.probe_tilt_fix === true && Math.abs(gNew.rear_downforce - 0.8) < 1e-6, `new balance: softer RWD tyres, rear downforce, probe fix ${JSON.stringify(gNew)}`);
await page.locator("#drift-lab-w button", { hasText: "step 2" }).click();
check((await abText()) === "balance: step 2", `step 2 preset: "${await abText()}"`);
await page.locator("#drift-lab-w button", { hasText: "for slow weight" }).click();
check((await abText()) === "balance: for slow weight" && (await page.evaluate(() => window.__wp.sim.gripBalance().rear_stiffness)) === 1, `slow-weight balance preset: "${await abText()}"`);
await page.locator("#drift-lab-w button", { hasText: "lock only (step 1)" }).click();
check((await abText()) === "balance: lock only" && (await page.evaluate(() => window.__wp.sim.gripBalance().rear_stiffness)) === 1, `lock only preset: "${await abText()}"`);
await page.locator("#drift-w-balance-ab").click(); // lock only → new
await page.evaluate(() => {
  const inputs = [...document.querySelectorAll("#drift-lab-w input[type=range]")];
  const s = inputs.find((i) => i.parentElement.textContent.includes("rear grip (μ)"));
  s.value = "0.9";
  s.dispatchEvent(new Event("input"));
});
check(Math.abs((await page.evaluate(() => window.__wp.sim.gripBalance().rear_mu)) - 0.9) < 1e-6, "rear grip slider sets the override");
await page.locator("#drift-w-balance-ab").click(); // custom → new
check((await page.evaluate(() => window.__wp.sim.gripBalance().rear_mu)) === 1 && (await abText()) === "balance: new", "A/B restores the new balance");
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
// Slow weight transfer preset (clip 4): sets the ride tuning (inertia too) and restarts the car.
await page.locator("#drift-lab-w button", { hasText: "slow weight (clip 4)" }).click();
await page.waitForTimeout(1500);
const sw = await page.evaluate(() => window.__wp.sim.rideTune());
check(sw.keep_ride_height === true && Math.abs(sw.roll_inertia - 2.4) < 1e-6 && (await sim()).name === "drift_lab_w", `slow weight preset ${JSON.stringify(sw)}`);
check(await page.locator("#drift-lab-w-keep-height").isChecked(), "keep ride height ticked by the preset");
await page.locator("#drift-lab-w button", { hasText: "lab default" }).click();
await page.waitForTimeout(1500);
check(Math.abs((await page.evaluate(() => window.__wp.sim.rideTune().arb)) - 0.25) < 1e-6, "lab default ride restored");
// Velocity-relative steering checkbox (default off) and TCS checkbox (default off).
check(!(await page.locator("#drift-lab-w-vr").isChecked()), "velocity-relative steering off by default");
await page.locator("#drift-lab-w-vr").check();
check((await page.evaluate(() => window.__wp.sim.plainSteer().velocity_relative)) === true, "VR checkbox → on");
await page.locator("#drift-lab-w-vr").uncheck();
check((await page.evaluate(() => window.__wp.sim.plainSteer().velocity_relative)) === false, "VR checkbox → off");
const player = () => page.evaluate(() => window.__wp.app.player);
check(!(await page.locator("#drift-lab-w-tcs").isChecked()) && !(await page.evaluate((id) => window.__wp.sim.assists(id).tcs, await player())), "TCS off by default");
await page.locator("#drift-lab-w-tcs").check();
check(await page.evaluate((id) => window.__wp.sim.assists(id).tcs, await player()), "TCS checkbox → assists.tcs on");
await page.locator("#drift-lab-w-tcs").uncheck();
check(!(await page.evaluate((id) => window.__wp.sim.assists(id).tcs, await player())), "TCS checkbox → off");
// Owner bug: floored from rest, TCS off, no steering, on the lab map → straight (< 3° of yaw),
// as the reference (clip 5). Restart the lab with the default car, then hold the throttle.
await page.selectOption("#drift-lab-w select", "parity_rwd_a");
await page.waitForFunction(() => window.__wp.sim.scenario().entities?.[0]?.car === "parity_rwd_a", null, { timeout: 30000 }).catch(() => {});
await page.selectOption("#drift-lab-w select", "parity_rwd_d");
await page.waitForFunction(() => window.__wp.sim.scenario().entities?.[0]?.car === "parity_rwd_d", null, { timeout: 30000 }).catch(() => {});
await page.waitForTimeout(1500);
const launch = async () => {
  const read = () => page.evaluate(() => Array.from(window.__wp.sim.plainReadout(window.__wp.app.player)));
  const r0 = await read();
  await page.keyboard.down("KeyW");
  let maxYaw = 0;
  let last = r0;
  for (let i = 0; i < 40; i++) {
    await page.waitForTimeout(125);
    last = await read();
    let d = last[9] - r0[9];
    d = ((d + 540) % 360) - 180;
    maxYaw = Math.max(maxYaw, Math.abs(d));
  }
  await page.keyboard.up("KeyW");
  return { maxYaw, kmh: last[7], gear: last[6] };
};
const l = await launch();
check(l.maxYaw < 3 && l.kmh > 20, `launch, floored from rest (spin model): yaw ${l.maxYaw.toFixed(2)}° at ${l.kmh.toFixed(0)} km/h, gear ${l.gear}`);
// For the record (not a check): the same launch on the tyre data — the owner's bug.
await page.locator("#drift-lab-w button", { hasText: "tyre data" }).click();
await page.selectOption("#drift-lab-w select", "parity_rwd_a");
await page.waitForFunction(() => window.__wp.sim.scenario().entities?.[0]?.car === "parity_rwd_a", null, { timeout: 30000 }).catch(() => {});
await page.selectOption("#drift-lab-w select", "parity_rwd_d");
await page.waitForFunction(() => window.__wp.sim.scenario().entities?.[0]?.car === "parity_rwd_d", null, { timeout: 30000 }).catch(() => {});
await page.waitForTimeout(1500);
const ld = await launch();
console.log(`INFO  launch on the tyre data (owner's bug): yaw ${ld.maxYaw.toFixed(1)}° at ${ld.kmh.toFixed(0)} km/h`);
await page.locator("#drift-lab-w button", { hasText: "spin model" }).first().click();
// Every parity car starts from the selector.
for (const car of ["parity_rwd_a", "parity_rwd_b", "parity_fwd_c", "shiro_turbo", "parity_rwd_d"]) {
  await page.selectOption("#drift-lab-w select", car);
  await page.waitForFunction((c) => window.__wp.sim.scenario().entities?.[0]?.car === c, car, { timeout: 30000 }).catch(() => {});
  sc = await sim();
  check(sc.name === "drift_lab_w" && sc.entities?.[0]?.car === car, `selector → ${car}`);
}
// From free drive, the testbed's "drift lab W" button lands in W (not the old lab C).
const freeDrive = async () => {
  await page.goto(`http://localhost:${PORT}/`);
  await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 90000 });
  await page.keyboard.press("Escape").catch(() => {});
};
await freeDrive();
check((await sim()).name !== "drift_lab_w", `page load = ${(await sim()).name}`);
check((await page.evaluate(() => window.__wp.sim.plainSteer())) === null, "free drive: plain steering off");
check((await page.evaluate(() => window.__wp.sim.gripBalance())) === null, "free drive: grip balance off");
check((await page.locator("#sc-drift-w").innerText()).trim() === "drift lab W", "button labelled drift lab W");
// The career garage can sit over free drive on load; the button's own click handler is
// what the owner hits from the testbed panel.
await page.evaluate(() => document.getElementById("sc-drift-w").click());
await page.waitForFunction(() => window.__wp.sim.scenario().name === "drift_lab_w", null, { timeout: 30000 }).catch(() => {});
sc = await sim();
check(sc.name === "drift_lab_w" && sc.entities?.[0]?.car === "parity_rwd_d", `button → ${sc.name} / ${sc.entities?.[0]?.car}`);
check((await page.evaluate(() => window.__wp.sim.plainSteer())) !== null, "button → plain steering on");
check((await page.title()) === "Drift Lab W", `button → title "${await page.title()}"`);
await page.waitForTimeout(500);
const visible = await page.evaluate(() => document.body.innerText);
check(!/drift lab c/i.test(visible) && (await page.getByText(/drift lab c/i).count()) === 0, "no visible 'Drift Lab C' anywhere (incl. shadow roots)");
check(/Drift Lab W · plain steering/.test(await page.locator("#drift-lab-w").innerText()), "button → W readout");
// The start menu's entry: "Drift Lab W" (no "Drift Lab C" entry) launches W.
await freeDrive();
await page.evaluate(() => window.__wp.menu.openMain());
await page.getByText("Drift Lab W", { exact: true }).first().waitFor({ timeout: 10000 }).catch(() => {});
// The menu renders in a shadow root: locators pierce it, body.innerText does not.
const wEntries = await page.getByText("Drift Lab W", { exact: true }).count();
const cEntries = await page.getByText(/drift lab c/i).count();
check(wEntries > 0 && cEntries === 0, `menu lists Drift Lab W (${wEntries}), no Drift Lab C (${cEntries})`);
await page.getByText("Drift Lab W", { exact: true }).first().click();
await page.waitForFunction(() => window.__wp.sim.scenario().name === "drift_lab_w", null, { timeout: 30000 }).catch(() => {});
sc = await sim();
check(sc.name === "drift_lab_w" && sc.entities?.[0]?.car === "parity_rwd_d", `menu → ${sc.name} / ${sc.entities?.[0]?.car}`);
check(errors.length === 0, `no page errors ${errors.join(" | ")}`);
await browser.close();
server.kill("SIGTERM");
process.exit(failures ? 1 : 0);
