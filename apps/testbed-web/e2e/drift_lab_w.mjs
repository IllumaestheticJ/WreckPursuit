// The handling test pad + tune panel (unify, 2026-10-21; was Drift Lab W): ?drift=w opens the
// pad with the parity car and the tune panel open; the panel changes the debug overrides; a held
// key moves the rack at the plain rate; every parity car starts from the car selector. Then the
// game itself: free drive, chase and races run the shipped handling (steering aids on the
// player's car only), F10 / ?tune=1 show the hidden panel, no separate lab entry points.
// `npm run build && node e2e/drift_lab_w.mjs` (PREVIEW=0: against `vite` dev instead of the
// built site; SHOT=path.png saves a screenshot).
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { chromiumPath } from "./chromium.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 5198;
// The pad with the hidden lab (?tune=1) on top: the flick / transition settings (row 137).
const URL = `http://localhost:${PORT}/?drift=w&tune=1`;
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
  executablePath: chromiumPath(),
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
check(ride && Math.abs(ride.arb - 1.5) < 1e-6 && Math.abs(ride.spring - 1.5) < 1e-6 && Math.abs(ride.damper_rebound - 2.1) < 1e-6, `ride default = the owner's (medium weight, springs / bars × 1.5) ${JSON.stringify(ride)}`);
{
  const g0 = await page.evaluate(() => window.__wp.sim.gripBalance());
  // Rows 141 / 142: the owner's step-14 tyres without the steering parts (no smart lock, no held-key cut).
  check(g0 && g0.lock_mode === "plain" && Math.abs(g0.rear_stiffness - 0.45) < 1e-6 && Math.abs(g0.front_stiffness - 0.6) < 1e-6 && g0.slide_lock === 1, `grip balance default = the owner's step 14 tyres, no steering parts ${JSON.stringify(g0)}`);
}
check(plain.counter_margin_deg === 0, `no countersteer limit by default, row 142 (${plain.counter_margin_deg})`);
const a = await page.evaluate(() => window.__wp.sim.assists(window.__wp.app?.player ?? window.__wp.sim.entities()[0]));
check((a.tcs === "Off" || a.tcs === false) && !a.esc && !a.auto_gears && a.auto_clutch === true, `assists TCS ${a.tcs} ESC ${a.esc} auto gears ${a.auto_gears} auto clutch ${a.auto_clutch}`);
check(await page.locator("#drift-w-tune").isVisible(), "tune button shown");
const sliders = await page.locator("#drift-lab-w input[type=range]").count();
check(sliders >= 18, `${sliders} sliders (steering + suspension + car / tyre)`);
check((await page.evaluate(() => window.__wp.sim.carTune()?.spin_model)) === true, "car / tyre experiments start at the spin model (W default)");
await page.locator("#drift-lab-w button", { hasText: "spin grip + damper" }).click();
const cand = await page.evaluate(() => window.__wp.sim.carTune());
check(Math.abs(cand.lat_under_spin - 0.15) < 1e-6 && Math.abs(cand.yaw_damping - 0.2) < 1e-6, `spin grip + damper ${JSON.stringify(cand)}`);
await page.locator("#drift-lab-w button", { hasText: "tyre data" }).click();
check((await page.evaluate(() => window.__wp.sim.carTune().yaw_damping)) === 0, "car data restores");
await page.locator("#drift-lab-w").getByRole("button", { name: "spin model (shipped)", exact: true }).click();
const spin = await page.evaluate(() => window.__wp.sim.carTune());
check(spin.spin_model === true && spin.yaw_damping === 0 && Math.abs(spin.spin_fx_slide - 0.958) < 1e-4, `spin model preset ${JSON.stringify(spin)}`);
await page.locator("#drift-lab-w button", { hasText: "spin + drift lock" }).click();
const sl = await page.evaluate(() => window.__wp.sim.carTune());
check(sl.spin_model === true && Math.abs(sl.drift_lock_extra + 0.5) < 1e-4 && Math.abs(sl.front_deep_falloff - 0.13) < 1e-4, `spin + drift lock preset ${JSON.stringify(sl)}`);
await page.locator("#drift-lab-w button", { hasText: "spin model v2 (ellipse)" }).click();
const v2 = await page.evaluate(() => ({ car: window.__wp.sim.carTune(), plain: window.__wp.sim.plainSteer() }));
check(v2.car.spin_model === true && Math.abs(v2.car.spin_ellipse - 0.7) < 1e-6 && Math.abs(v2.plain.swing_rate - 2.08) < 1e-6, `spin model v2 preset ${JSON.stringify(v2)}`);
await page.locator("#drift-lab-w button", { hasText: "lab · transition: swing rack (test)" }).click();
await page.waitForTimeout(300);
const tr = await page.evaluate(() => ({ plain: window.__wp.sim.plainSteer(), ride: window.__wp.sim.rideTune(), grip: window.__wp.sim.gripBalance() }));
check(Math.abs(tr.plain.swing_rate - 2.5) < 1e-6 && Math.abs(tr.ride.yaw_inertia - 1.5) < 1e-6 && tr.grip.lock_mode === "smart" && Math.abs(tr.grip.rear_stiffness - 0.45) < 1e-6, `transition preset (balance step 14) ${JSON.stringify(tr)}`);
await page.locator("#drift-lab-w button", { hasText: "drift: spin + VR (test)" }).click();
const svr = await page.evaluate(() => ({ car: window.__wp.sim.carTune(), plain: window.__wp.sim.plainSteer() }));
check(svr.car.spin_model === true && svr.plain.velocity_relative === true && Math.abs(svr.plain.align_gain - 0.5) < 1e-6, `drift: spin + VR preset ${JSON.stringify(svr)}`);
await page.evaluate(() => window.__wp.sim.plainSteerSet(JSON.stringify(window.__wp.sim.plainSteerDefaults())));
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
// The transition preset above left balance step 14: A/B goes back to new.
check((await abText()) === "balance: step 14", `after the transition preset: "${await abText()}"`);
await page.locator("#drift-w-balance-ab").click();
check((await abText()) === "balance: shipped", `A/B button reads "${await abText()}" (balance: shipped)`);
await page.locator("#drift-w-balance-ab").click();
check((await page.evaluate(() => window.__wp.sim.gripBalance()?.lock_mode)) === "plain", `A/B → before (plain lock, tyre data): ${JSON.stringify(await page.evaluate(() => window.__wp.sim.gripBalance()))}`);
check((await abText()) === "balance: before", `A/B button reads "${await abText()}" (balance: before)`);
await page.locator("#drift-w-balance-ab").click();
check(Math.abs((await page.evaluate(() => window.__wp.sim.gripBalance()?.rear_stiffness)) - 0.45) < 1e-6, "A/B → new again (the step-14 rear tyres)");
const gNew = await page.evaluate(() => window.__wp.sim.gripBalance());
check(Math.abs(gNew.rear_stiffness - 0.45) < 1e-6 && Math.abs(gNew.front_stiffness - 0.6) < 1e-6 && gNew.tyres_rwd_only === true && gNew.probe_tilt_fix === true && Math.abs(gNew.rear_downforce - 0.8) < 1e-6, `shipped balance: the owner's step 14, rear downforce, probe fix ${JSON.stringify(gNew)}`);
// Held-key drift lab preset (held_search candidate D, 2026-10-22; ?tune=1 only).
await page.locator("#drift-lab-w button", { hasText: "lab · held-key drift" }).click();
await page.waitForTimeout(300);
{
  const hk = await page.evaluate(() => ({ car: window.__wp.sim.carTune(), grip: window.__wp.sim.gripBalance(), ride: window.__wp.sim.rideTune(), steer: window.__wp.sim.plainSteer() }));
  check(Math.abs(hk.car.spin_b0 - 11.84) < 1e-4 && hk.car.lock === 1 && Math.abs(hk.grip.rear_stiffness - 0.634) < 1e-6 && Math.abs(hk.ride.yaw_inertia - 1.385) < 1e-6 && Math.abs(hk.steer.sens_ref_kmh - 49.2) < 1e-4 && hk.grip.lock_mode === "plain", `held-key drift lab preset ${JSON.stringify({ b0: hk.car.spin_b0, lock: hk.car.lock, rear: hk.grip.rear_stiffness, yaw: hk.ride.yaw_inertia, sens: hk.steer.sens_ref_kmh })}`);
}
// Back to the shipped tyres / balance / ride for the checks below.
await page.evaluate(() => {
  window.__wp.sim.carTuneSet("");
  window.__wp.sim.gripBalanceSet("");
  window.__wp.sim.rideTuneSet("");
  window.__wp.sim.plainSteerSet("");
  for (const k of ["wp.tune.car.v1", "wp.tune.grip.v1", "wp.tune.ride.v1", "wp.tune.steer.v1"]) localStorage.removeItem(k);
});
await page.locator("#drift-lab-w button", { hasText: "balance: step 14" }).click();
check((await abText()) === "balance: step 14" && Math.abs((await page.evaluate(() => window.__wp.sim.gripBalance().rear_stiffness)) - 0.45) < 1e-6, `step 14 preset: "${await abText()}"`);
await page.locator("#drift-w-balance-ab").click();
await page.locator("#drift-w-balance-grip").click();
{
  const gg = await page.evaluate(() => window.__wp.sim.gripBalance());
  check((await abText()) === "balance: grip (test)" && Math.abs(gg.rear_stiffness - 0.8) < 1e-6 && Math.abs(gg.rear_mu - 0.96) < 1e-6 && Math.abs(gg.rear_lat_c - 1.0) < 1e-6, `grip (test) preset: "${await abText()}" ${JSON.stringify(gg)}`);
}
// The owner's rear A/B presets (lead, 2026-10-21).
for (const [id, k] of [["drift-w-rear-50", 0.5], ["drift-w-rear-55", 0.55]]) {
  await page.locator(`#${id}`).click();
  const g = await page.evaluate(() => window.__wp.sim.gripBalance());
  check(Math.abs(g.rear_stiffness - k) < 1e-6 && Math.abs(g.front_stiffness - 0.6) < 1e-6 && (await abText()) === `balance: rear ×${k.toFixed(2)}`, `rear ×${k} preset: "${await abText()}"`);
}
await page.locator("#drift-lab-w").getByRole("button", { name: "step 2", exact: true }).click();
check((await abText()) === "balance: step 2", `step 2 preset: "${await abText()}"`);
await page.locator("#drift-w-drift-best").click();
const db = await page.evaluate(() => ({ g: window.__wp.sim.gripBalance(), p: window.__wp.sim.plainSteer() }));
check((await abText()) === "balance: drift best" && Math.abs(db.g.rear_stiffness - 0.45) < 1e-6 && Math.abs(db.g.rear_downforce - 0.8) < 1e-6 && db.p.velocity_relative === true && Math.abs(db.p.slide_key_gain - 0.31) < 1e-6, `drift: best (test) sets the balance and the VR steering ${JSON.stringify(db)}`);
await page.evaluate(() => {
  const vr = document.getElementById("drift-lab-w-vr");
  if (vr.checked) vr.click();
});
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
check((await page.evaluate(() => window.__wp.sim.gripBalance().rear_mu)) === 1 && (await abText()) === "balance: shipped", "A/B restores the shipped balance");
// Steering slider → override.
await page.evaluate(() => {
  const inputs = [...document.querySelectorAll("#drift-lab-w input[type=range]")];
  const s = inputs.find((i) => i.parentElement.textContent.includes("full lock time"));
  s.value = "0.4";
  s.dispatchEvent(new Event("input"));
});
check(Math.abs((await page.evaluate(() => window.__wp.sim.plainSteer().t_full_s)) - 0.4) < 1e-6, "full lock time slider sets the override");
await page.locator("#drift-lab-w").getByRole("button", { name: "steering defaults", exact: true }).click();
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
check(Math.abs((await page.evaluate(() => window.__wp.sim.rideTune().arb)) - 0.25) < 1e-6, "lab default ride (before) set");
// Step 10 A/B: "before" (no countersteer limit, lab ride) vs "new" (limit 0.25°, the firm ride since step 20).
await page.locator("#drift-lab-w-before").click();
await page.waitForTimeout(1500);
{
  const b = await page.evaluate(() => [window.__wp.sim.plainSteer().counter_margin_deg, window.__wp.sim.rideTune().spring]);
  check(b[0] === 0 && Math.abs(b[1] - 0.9) < 1e-6, `"before" → limit off, lab ride (${b})`);
}
await page.locator("#drift-lab-w-new").click();
await page.waitForTimeout(1500);
{
  const n = await page.evaluate(() => [window.__wp.sim.plainSteer().counter_margin_deg, window.__wp.sim.rideTune().spring]);
  check(n[0] === 0 && Math.abs(n[1] - 1.5) < 1e-6 && (await sim()).name === "drift_lab_w", `"new" → the shipped steering (no limit since row 142), firm ride (${n})`);
}
// Step 11 catch A/B (owner: catch over-rotations): default new, "before" turns the directional
// limit / extra lock / fast rack off.
{
  const ps = () => page.evaluate(() => window.__wp.sim.plainSteer());
  const d = await ps();
  // Row 142: the extra lock and the fast rack are lab-only now (off for players).
  check(d.counter_closing_only === true && d.drift_lock_extra === 0 && d.over_rate === 1, `catch default: directional limit, no extra lock, no fast rack (${d.counter_closing_only}, ${d.drift_lock_extra}, ${d.over_rate})`);
  await page.locator("#drift-lab-w-catch-before").click();
  check((await ps()).counter_closing_only === false, '"catch: before" → off');
  await page.locator("#drift-lab-w-catch-new").click();
  check((await ps()).counter_closing_only === true, '"catch: new" → on');
  // Step 12 transition rack: off by default since row 142 (lab A/B only), the checkbox reaches the sim.
  check((await ps()).trans_follow === false && !(await page.locator("#drift-lab-w-trans").isChecked()), "transition rack off by default (row 142)");
  await page.locator("#drift-lab-w-trans").check();
  check((await ps()).trans_follow === true, "lab transition checkbox → on");
  await page.locator("#drift-lab-w-trans").uncheck();
  check((await ps()).trans_follow === false, "lab transition checkbox → off");
}
// Velocity-relative steering checkbox (default off) and TCS checkbox (default off).
check(!(await page.locator("#drift-lab-w-vr").isChecked()), "velocity-relative steering off by default");
await page.locator("#drift-lab-w-vr").check();
check((await page.evaluate(() => window.__wp.sim.plainSteer().velocity_relative)) === true, "VR checkbox → on");
await page.locator("#drift-lab-w-vr").uncheck();
check((await page.evaluate(() => window.__wp.sim.plainSteer().velocity_relative)) === false, "VR checkbox → off");
const player = () => page.evaluate(() => window.__wp.app.player);
{
  const lv = async () => page.evaluate((id) => { const a = window.__wp.sim.assists(id); return [a.tcs, a.abs]; }, await player());
  const [t0, a0] = await lv();
  check((await page.locator("#drift-lab-w-tcs").inputValue()) === "Off" && t0 === "Off" && a0 === "Full", `TCS Off / ABS Full by default (${t0} / ${a0})`);
  await page.locator("#drift-lab-w-tcs").selectOption("Half");
  await page.locator("#drift-lab-w-abs").selectOption("Off");
  const [t1, a1] = await lv();
  check(t1 === "Half" && a1 === "Off", `level selects → assists (${t1} / ${a1})`);
  await page.locator("#drift-lab-w-tcs").selectOption("Off");
  await page.locator("#drift-lab-w-abs").selectOption("Full");
  check((await lv()).join() === "Off,Full", "level selects back to Off / Full");
  // Handbrake pivot aid knobs (round12 row 109) reach the drift tuning.
  const pivot0 = await page.evaluate(() => window.__wp.sim.driftTuning().hb_pivot_accel);
  check(pivot0 === 7, `pivot aid default 7 rad/s² (${pivot0})`);
  await page.locator("#drift-lab-w-hb_pivot_accel").fill("0");
  check((await page.evaluate(() => window.__wp.sim.driftTuning().hb_pivot_accel)) === 0, "pivot slider → drift tuning 0");
  await page.locator("#drift-lab-w-hb_pivot_accel").fill("7");
}
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
// Owner bug (2026-10-20): "rpm stuck / flat" in 3rd with the clutch set to manual — the lab
// forced the automatic clutch. The testbed's clutch select must win (also after the lab
// restarts), show in the readout, and a 3rd-gear pull must rev with road speed.
{
  await page.evaluate(() => {
    const sel = document.getElementById("clutch-mode");
    sel.value = "manual";
    sel.dispatchEvent(new Event("change"));
  });
  check((await page.evaluate((id) => window.__wp.sim.assists(id).auto_clutch, await player())) === false, "clutch select manual → assists.auto_clutch off");
  // The owner's flow: the choice is saved, then the lab starts (it used to force auto here).
  await page.reload();
  await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 90000 });
  const pid = await player();
  check((await page.evaluate((id) => window.__wp.sim.assists(id).auto_clutch, pid)) === false, "lab start keeps the manual clutch");
  const pullRead = () => page.evaluate((id) => Array.from(window.__wp.sim.plainReadout(id)), pid);
  await page.waitForTimeout(300);
  check((await pullRead())[10] === 0, "readout: clutch manual");
  check(/clutch manual/.test(await page.locator("#drift-lab-w").innerText()), "readout text shows clutch manual");
  // Get rolling in 3rd on the automatic clutch (from rest the manual clutch with the pedal up
  // stalls, as it should), then the manual clutch for 3 s of full throttle in 3rd.
  const setClutch = (v) =>
    page.evaluate((v) => {
      const sel = document.getElementById("clutch-mode");
      sel.value = v;
      sel.dispatchEvent(new Event("change"));
    }, v);
  await setClutch("auto");
  await page.keyboard.down("KeyW");
  for (let i = 0; i < 60 && (await pullRead())[6] < 3; i++) {
    await page.waitForTimeout(400);
    const r = await pullRead();
    if (r[6] >= 1 && r[13] > 4500) await page.keyboard.press("ShiftLeft");
  }
  await page.keyboard.up("KeyW");
  await setClutch("manual");
  await page.keyboard.down("KeyW");
  await page.waitForTimeout(500);
  const s0 = await pullRead();
  await page.waitForTimeout(3000);
  const s1 = await pullRead();
  await page.keyboard.up("KeyW");
  check(s0[10] === 0 && s0[6] === 3 && s1[6] === 3 && s1[13] > s0[13] + 400 && s1[7] > s0[7] + 8, `3rd-gear pull, manual clutch: ${s0[13].toFixed(0)} → ${s1[13].toFixed(0)} rpm, ${s0[7].toFixed(0)} → ${s1[7].toFixed(0)} km/h, wheels ${s0[14].toFixed(0)} → ${s1[14].toFixed(0)} rpm (gear ${s0[6]} → ${s1[6]})`);
  await page.evaluate(() => {
    const sel = document.getElementById("clutch-mode");
    sel.value = "auto";
    sel.dispatchEvent(new Event("change"));
  });
}
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
await page.locator("#drift-lab-w").getByRole("button", { name: "spin model (shipped)", exact: true }).first().click();
// Every parity car starts from the selector.
for (const car of ["parity_rwd_a", "parity_rwd_b", "parity_fwd_c", "shiro_turbo", "parity_rwd_d"]) {
  await page.selectOption("#drift-lab-w select", car);
  await page.waitForFunction((c) => window.__wp.sim.scenario().entities?.[0]?.car === c, car, { timeout: 30000 }).catch(() => {});
  sc = await sim();
  check(sc.name === "drift_lab_w" && sc.entities?.[0]?.car === car, `selector → ${car}`);
}
// Unify (lead, 2026-10-21): the W handling is the game's handling. Free drive, night run and
// races run it with no panel and no saved overrides: shipped physics on every car, the plain
// steering on the player's car only. The tune panel is hidden (F10 / ?tune=1).
const freeDrive = async (q = "") => {
  await page.goto(`http://localhost:${PORT}/${q}`);
  await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 90000 });
  await page.keyboard.press("Escape").catch(() => {});
};
await page.evaluate(() => {
  for (const k of Object.keys(localStorage)) if (k.startsWith("wp.tune.")) localStorage.removeItem(k);
});
await freeDrive();
check((await sim()).name !== "drift_lab_w", `page load = ${(await sim()).name}`);
const handling = () =>
  page.evaluate(() => {
    const s = window.__wp.sim;
    const p = window.__wp.app.player;
    const others = s.entities().filter((e) => e !== p);
    return {
      name: s.scenario().name,
      plain: s.plainSteer() !== null,
      spin: s.carTune()?.spin_model === true,
      grip: s.gripBalance()?.lock_mode,
      firm: Math.abs((s.rideTune()?.spring ?? 0) - 1.5) < 1e-6,
      player: s.playerInput(p),
      others: others.length,
      othersPlayer: others.filter((e) => s.playerInput(e)).length,
      panel: getComputedStyle(document.getElementById("drift-lab-w")).display,
      title: document.title,
    };
  });
{
  const w = await handling();
  check(w.plain && w.spin && w.grip === "plain" && w.firm && w.player, `free drive: shipped handling, player keyboard rack (${JSON.stringify(w)})`);
  check(w.panel === "none" && w.title !== "Drift Lab W", `free drive: tune panel hidden (${w.panel})`);
  check((await page.locator("#sc-drift-w").count()) === 0, "no separate drift lab W button");
  // (The load's Escape may have opened the pause menu, which keeps the keys.)
  await page.evaluate(() => {
    if (window.__wp.menu?.isOpen) window.__wp.menu.close();
  });
  await page.keyboard.press("F10");
  check((await page.evaluate(() => getComputedStyle(document.getElementById("drift-lab-w")).display)) === "block", "F10 shows the tune panel");
  await page.keyboard.press("F10");
  check((await page.evaluate(() => getComputedStyle(document.getElementById("drift-lab-w")).display)) === "none", "F10 hides it again");
}
// Row 137: the pad alone (no ?tune=1) has no flick / transition settings.
await page.goto(`http://localhost:${PORT}/?drift=w`);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 90000 });
await page.waitForTimeout(1000);
{
  const labRows = await page.evaluate(() => [...document.querySelectorAll("#drift-lab-w *")].filter((e) => e.children.length === 0 && (e.textContent ?? "").includes("lab · ")).length);
  check(labRows === 0 && (await page.locator("#drift-lab-w-trans").count()) === 0, `pad without ?tune=1: no flick / transition settings (${labRows})`);
}
await freeDrive("?tune=1");
check((await page.evaluate(() => getComputedStyle(document.getElementById("drift-lab-w")).display)) === "block", "?tune=1 shows the tune panel");
check((await sim()).name !== "drift_lab_w", "?tune=1 keeps the session (no separate lab)");
{
  // The pre-unify A/B checkbox: legacy = the old stock path; back off = shipped.
  await page.evaluate(() => {
    const b = document.getElementById("drift-lab-w-legacy");
    if (b && !b.checked) b.click();
  });
  const legacy = await page.evaluate(() => ({ l: window.__wp.sim.labScopeLegacy(), car: window.__wp.sim.carTune(), plain: window.__wp.sim.plainSteer() }));
  check(legacy.l && legacy.car === null && legacy.plain === null, `pre-unify A/B on: data tyres, no plain steering (${JSON.stringify(legacy)})`);
  await page.evaluate(() => document.getElementById("drift-lab-w-legacy").click());
  check((await page.evaluate(() => window.__wp.sim.labScopeLegacy())) === false, "pre-unify A/B off again");
  // "Reset all to defaults": a saved override goes away, the shipped handling is back.
  await page.evaluate(() => {
    window.__wp.sim.rideTuneSet(JSON.stringify({ spring: 0.7 }));
    localStorage.setItem("wp.tune.ride.v1", JSON.stringify({ spring: 0.7 }));
  });
  await page.evaluate(() => document.getElementById("drift-lab-w-reset-all").click());
  const reset = await page.evaluate(() => ({ ride: window.__wp.sim.rideTune(), saved: Object.keys(localStorage).filter((k) => k.startsWith("wp.tune.")) }));
  check(Math.abs(reset.ride.spring - 1.5) < 1e-6 && reset.saved.length === 0, `reset all → shipped ride, no saved tune keys (${JSON.stringify(reset)})`);
  // "Copy tune JSON": everything in effect.
  const tj = JSON.parse(await page.evaluate(() => window.__wp.tunePanel.tuneJson()));
  check(tj.legacy === false && tj.steer && tj.ride && tj.tyres && tj.grip && tj.assists, `tune JSON has steering / ride / tyres / grip / assists (${Object.keys(tj).join(",")})`);
}
// The start menu has no Drift Lab W / C entry any more.
await freeDrive();
await page.evaluate(() => window.__wp.menu.openMain());
await page.getByText("Free drive", { exact: true }).first().waitFor({ timeout: 10000 }).catch(() => {});
{
  const wEntries = await page.getByText("Drift Lab W", { exact: true }).count();
  const cEntries = await page.getByText(/drift lab c/i).count();
  check(wEntries === 0 && cEntries === 0, `menu: no Drift Lab W (${wEntries}) / C (${cEntries}) entries`);
}
// Night run: the same physics on every car; the steering aids only on the player's car.
{
  await page.goto(`http://localhost:${PORT}/?chase=1`);
  await page.waitForFunction(() => window.__wp?.ready === true && window.__wp.sim.scenario().name === "night_run", null, { timeout: 90000 }).catch(() => {});
  await page.waitForTimeout(3000);
  const c = await handling();
  check(c.name === "night_run" && c.plain && c.spin && c.firm && c.player, `chase: shipped handling on the player (${JSON.stringify(c)})`);
  check(c.others === 0 || c.othersPlayer === 0, `chase: ${c.others} police / traffic cars without the player steering aids (${c.othersPlayer})`);
  // A street race: AI racers drive the same physics with their own steering.
  await page.goto(`http://localhost:${PORT}/?race=grid_highway_sprint`);
  await page.waitForFunction(() => window.__wp?.ready === true && window.__wp.sim.scenario().name.startsWith("race_"), null, { timeout: 90000 }).catch(() => {});
  await page.waitForTimeout(1500);
  const r = await handling();
  check(r.name.startsWith("race_") && r.player && r.others > 0, `race: player steering aids on the player (${JSON.stringify(r)})`);
  // The racers are marked AI when the race starts (the countdown); before that they are spawned
  // as player-kind. Report, then check after a few seconds.
  await page.waitForTimeout(6000);
  const r2 = await handling();
  check(r2.othersPlayer === 0, `race: ${r2.others} AI racers without the player steering aids (${r2.othersPlayer})`);
}
check(errors.length === 0, `no page errors ${errors.join(" | ")}`);
await browser.close();
server.kill("SIGTERM");
process.exit(failures ? 1 : 0);
