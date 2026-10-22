// Per-device input shaping (game menu → Settings → Controls, and the testbed controls panel) in
// headless Chromium (Playwright). Usage: npm run e2e:controls (needs `npm run wasm` first).
// Env: CHROMIUM=/path/to/chrome, SHOTS=dir to write screenshots of the Controls section.
//
// Checks: keyboard defaults 0 / 100 / 100 / 75 %; the player's AssistProfile gets the device's
// steering speed sensitivity (keyboard 0.75, gamepad 0.70, wheel 0, 0 with steering help off);
// a slider change persists across a reload; "Test input" lets driving keys move the test bars
// (steer ramps to ~100 % and back, throttle / brake respond) while menu navigation is ignored and
// Esc ends the test; saturation 50 % and dead zone 30 % change the processed value at a fixed
// time; the gamepad pipeline applies the gamepad profile; the controls panel edits the same
// values; the Rookie / Street / Sim highlight survives a speed-sensitivity change.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5203;
const URL = `http://localhost:${PORT}/`;
const shots = process.env.SHOTS;
if (shots) mkdirSync(shots, { recursive: true });

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
const near = (a, b, tol = 0.011) => Math.abs(a - b) <= tol;

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const context = await browser.newContext({ viewport: { width: 1280, height: 720 } });
// A fake standard pad (at rest unless a test moves it) so the gamepad / wheel profiles can be
// checked; `__pad.set({axes, id})` changes it.
await context.addInitScript(() => {
  const pad = {
    id: "WreckPursuit fake pad (STANDARD GAMEPAD)",
    index: 0,
    connected: true,
    mapping: "standard",
    timestamp: 0,
    axes: [0, 0, 0, 0],
    buttons: Array.from({ length: 17 }, () => ({ pressed: false, touched: false, value: 0 })),
  };
  Object.defineProperty(Navigator.prototype, "getGamepads", { configurable: true, value: () => [pad, null, null, null] });
  window.__pad = {
    set({ axes, id }) {
      if (axes) pad.axes = axes;
      if (id) pad.id = id;
      pad.timestamp++;
    },
  };
});
const page = await context.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));

const ready = () => page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
const frames = (n) =>
  page.evaluate(
    (k) =>
      new Promise((r) => {
        let i = 0;
        const f = () => (++i >= k ? r() : requestAnimationFrame(f));
        requestAnimationFrame(f);
      }),
    n,
  );
const speedSens = () => page.evaluate(() => window.__wp.sim.assists(window.__wp.app.player).steer_speed_sensitivity);
const slider = (key) =>
  page.evaluate((k) => {
    const el = document.getElementById(`gm-ctl-${k}`);
    return { value: Number(el.value), text: el.nextElementSibling?.textContent ?? "" };
  }, key);
const bar = (ch) => page.evaluate((c) => Number(document.querySelector(`#wp-game-menu [data-bar="${c}"]`)?.dataset.value ?? NaN), ch);
/** Set a menu slider through its own input event (as dragging it would). */
const setSlider = (key, v) =>
  page.evaluate(
    ([k, x]) => {
      const el = document.getElementById(`gm-ctl-${k}`);
      el.value = String(x);
      el.dispatchEvent(new Event("input", { bubbles: true }));
    },
    [key, v],
  );
const openSettings = async () => {
  await page.evaluate(() => {
    window.__wp.menu.openPause();
    window.__wp.menu.openSettings("pause");
  });
  await frames(2);
};
const waitBar = (ch, pred, ms = 8000) =>
  page
    .waitForFunction(([c, f]) => new Function("v", `return ${f}`)(Number(document.querySelector(`#wp-game-menu [data-bar="${c}"]`)?.dataset.value ?? NaN)), [ch, pred], { timeout: ms })
    .then(() => true)
    .catch(() => false);

// ----- fresh viewer: defaults ----------------------------------------------------------------
await page.goto(`${URL}?dev=1`);
await ready();
await page.evaluate(() => ["wp.gameSettings.v1", "wp.assistPreset", "wp.assistOverrides"].forEach((k) => localStorage.removeItem(k)));
await page.reload();
await ready();
await frames(3);
check(near(await speedSens(), 0.75, 1e-6), `keyboard: player's steer_speed_sensitivity = 0.75 (${await speedSens()})`);

await openSettings();
const dz = await slider("steerDeadzone");
const sat = await slider("steerSaturation");
const sens = await slider("steerSensitivity");
const spd = await slider("steerSpeedSensitivity");
check(
  dz.value === 0 && sat.value === 1 && sens.value === 1 && spd.value === 0.75 && [dz, sat, sens, spd].map((s) => s.text).join(" ") === "0% 100% 100% 75%",
  `keyboard defaults shown: ${[dz, sat, sens, spd].map((s) => s.text).join(" / ")}`,
);
const pedals = await page.evaluate(() =>
  ["throttle", "brake", "clutch"].map((p) => [document.getElementById(`gm-ctl-${p}Deadzone`).value, document.getElementById(`gm-ctl-${p}Saturation`).value].join("/")),
);
check(pedals.every((p) => p === "0/1"), `pedal dead zone 0 % / saturation 100 % (${pedals.join(" ")})`);
const device = await page.evaluate(() => document.getElementById("gm-ctl-device").textContent);
check(device.includes("Keyboard"), `device cycle shows Keyboard (${device})`);
if (shots) await page.locator(".wp-gm-settings").screenshot({ path: join(shots, "controls_settings.png") });

// ----- test mode -----------------------------------------------------------------------------
await page.click("#gm-ctl-test");
await frames(2);
check(await page.evaluate(() => window.__wp.menu.testingInput), "Test input on");
await page.keyboard.down("ArrowRight");
const full = await waitBar("steer", "v >= 0.98");
check(full, `hold ArrowRight → steering bar ~100 % (${await bar("steer")})`);
await page.keyboard.up("ArrowRight");
check(await waitBar("steer", "v === 0"), `release → steering bar 0 (${await bar("steer")})`);
await page.keyboard.down("ArrowUp");
check(await waitBar("throttle", "v >= 0.98"), `ArrowUp → throttle bar ~100 % (${await bar("throttle")})`);
await page.keyboard.up("ArrowUp");
check(await waitBar("throttle", "v === 0"), "throttle back to 0");
await page.keyboard.down("ArrowDown");
check(await waitBar("brake", "v >= 0.98"), `ArrowDown → brake bar ~100 % (${await bar("brake")})`);
await page.keyboard.up("ArrowDown");
check(await waitBar("brake", "v === 0"), "brake back to 0");
const st = await page.evaluate(() => ({ screen: window.__wp.menu.screen, testing: window.__wp.menu.testingInput, focus: document.activeElement?.id ?? "" }));
check(st.screen === "settings" && st.testing && !st.focus.startsWith("gm-"), `navigation keys ignored while testing (${JSON.stringify(st)})`);
if (shots) await page.locator(".wp-gm-bars").screenshot({ path: join(shots, "controls_bars.png") });
await page.keyboard.press("Escape");
await frames(2);
const afterEsc = await page.evaluate(() => ({ screen: window.__wp.menu.screen, testing: window.__wp.menu.testingInput }));
check(afterEsc.screen === "settings" && !afterEsc.testing, `Esc ends the test, settings stay open (${JSON.stringify(afterEsc)})`);

// ----- persistence + speed sensitivity -------------------------------------------------------
await page.focus("#gm-ctl-steerSpeedSensitivity");
for (let i = 0; i < 5; i++) await page.keyboard.press("ArrowLeft");
await frames(2);
const s50 = await slider("steerSpeedSensitivity");
check(near(s50.value, 0.5, 1e-6) && s50.text === "50%", `ArrowLeft ×5 → speed sensitivity 50 % (${s50.text})`);
check(near(await speedSens(), 0.5, 1e-6), `player's steer_speed_sensitivity follows (${await speedSens()})`);
await page.reload();
await ready();
await frames(3);
await openSettings();
const kept = await slider("steerSpeedSensitivity");
check(near(kept.value, 0.5, 1e-6) && kept.text === "50%", `speed sensitivity 50 % survives a reload (${kept.text})`);
check(near(await speedSens(), 0.5, 1e-6), `…and is applied after the reload (${await speedSens()})`);
// Steering help off → 0.
await page.evaluate(() => window.__wp.menu.update({ assists: { ...window.__wp.menu.settings.assists, steerHelp: false } }));
check((await speedSens()) === 0, `steering help off → steer_speed_sensitivity 0 (${await speedSens()})`);
await page.evaluate(() => window.__wp.menu.update({ assists: { ...window.__wp.menu.settings.assists, steerHelp: true } }));
check(near(await speedSens(), 0.5, 1e-6), `steering help on again → 0.5 (${await speedSens()})`);

// ----- saturation / dead zone at a fixed time (deterministic probe clock) ---------------------
await page.evaluate(() => window.__wp.menu.close());
await frames(2);
/** Processed steer after holding ArrowRight for exactly 0.1 s of probe time (key ramp: 0.4). */
const tapSteer = async () => {
  const T = await page.evaluate(() => performance.now() + 1e7);
  await page.evaluate((t) => {
    window.__wp.inputProbe(t);
    window.__wp.inputProbe(t + 1000); // settle every ramp at rest
  }, T);
  await page.keyboard.down("ArrowRight");
  const v = await page.evaluate((t) => window.__wp.inputProbe(t + 1100).steer, T);
  await page.keyboard.up("ArrowRight");
  await page.evaluate((t) => window.__wp.inputProbe(t + 3000), T);
  return v;
};
const setKb = (patch) => page.evaluate((p) => window.__wp.menu.update({ controls: { ...window.__wp.menu.settings.controls, keyboard: { ...window.__wp.menu.settings.controls.keyboard, ...p } } }), patch);
const base = await tapSteer();
await setKb({ steerSaturation: 0.5 });
const sat50 = await tapSteer();
check(near(base, 0.4) && near(sat50, 0.8) && sat50 > base, `saturation 50 % → bar fuller at the same time (${base.toFixed(3)} → ${sat50.toFixed(3)})`);
await setKb({ steerSaturation: 1, steerDeadzone: 0.3 });
const dz30 = await tapSteer();
check(dz30 < base && near(dz30, 0.1 / 0.7), `dead zone 30 % → a short tap shows less (${base.toFixed(3)} → ${dz30.toFixed(3)})`);
await setKb({ steerDeadzone: 0 });

// ----- gamepad / wheel profiles --------------------------------------------------------------
await page.evaluate(() => {
  const c = window.__wp.menu.settings.controls;
  window.__wp.menu.update({ controls: { ...c, gamepad: { ...c.gamepad, steerSaturation: 0.5 } } });
  window.__pad.set({ axes: [0.5, 0, 0, 0] });
});
await page.waitForFunction(() => window.__wp.app.source === "Gamepad", null, { timeout: 10000 }).catch(() => {});
await frames(3);
const padIn = await page.evaluate(() => ({ steer: window.__wp.app.input.steer, dev: window.__wp.activeDevice(), dz: window.__wp.bindings().padDeadzone, curve: window.__wp.bindings().padCurve }));
// Pad pipeline: deadzone, curve, then the gamepad profile (saturation 50 % doubles it).
const wantPad = Math.min(1, ((((0.5 - padIn.dz) / (1 - padIn.dz)) ** padIn.curve) / 0.5));
check(padIn.dev === "gamepad" && near(padIn.steer, wantPad, 0.002), `gamepad steer through the gamepad profile (${padIn.steer.toFixed(3)} ≈ ${wantPad.toFixed(3)})`);
check(near(await speedSens(), 0.7, 1e-6), `gamepad: steer_speed_sensitivity 0.70 (${await speedSens()})`);
await page.evaluate(() => window.__pad.set({ id: "Fake Racing Wheel (STANDARD GAMEPAD)" }));
await page.waitForFunction(() => window.__wp.activeDevice() === "wheel", null, { timeout: 10000 }).catch(() => {});
await frames(3);
check((await page.evaluate(() => window.__wp.activeDevice())) === "wheel" && (await speedSens()) === 0, `a pad named "wheel" uses the wheel profile: steer_speed_sensitivity 0 (${await speedSens()})`);
await page.evaluate(() => window.__pad.set({ axes: [0, 0, 0, 0], id: "WreckPursuit fake pad (STANDARD GAMEPAD)" }));
await page.keyboard.down("KeyW");
await page.waitForFunction(() => window.__wp.app.source === "Keyboard", null, { timeout: 10000 }).catch(() => {});
await page.keyboard.up("KeyW");
await frames(3);
check(near(await speedSens(), 0.5, 1e-6), `back on the keyboard → 0.5 again (${await speedSens()})`);

// ----- controls panel edits the same values ---------------------------------------------------
await page.evaluate(() => window.__wp.controls.open());
await frames(2);
const panelV = await page.evaluate(() => Number(document.getElementById("shape-steerSpeedSensitivity").value));
check(near(panelV, 0.5, 1e-6), `controls panel shows the menu's value (${panelV})`);
await page.evaluate(() => {
  const el = document.getElementById("shape-steerSpeedSensitivity");
  el.value = "0.6";
  el.dispatchEvent(new Event("input", { bubbles: true }));
});
const menuV = await page.evaluate(() => window.__wp.menu.settings.controls.keyboard.steerSpeedSensitivity);
check(near(menuV, 0.6, 1e-6) && near(await speedSens(), 0.6, 1e-6), `panel slider → game menu settings + assists (${menuV}, ${await speedSens()})`);
await page.keyboard.down("ArrowLeft");
const panelBar = await page
  .waitForFunction(() => Number(document.querySelector('#controls-panel [data-bar="steer"]')?.dataset.value) <= -0.98, null, { timeout: 8000 })
  .then(() => true)
  .catch(() => false);
check(panelBar, "panel test bar: hold ArrowLeft → steering −100 %");
await page.keyboard.up("ArrowLeft");
if (shots) await page.locator("#controls-panel").screenshot({ path: join(shots, "controls_panel.png") });
await page.evaluate(() => window.__wp.controls.close());

// ----- preset highlight ----------------------------------------------------------------------
await page.click('button.preset[data-p="Street"]');
const lit = () => page.evaluate(() => [...document.querySelectorAll("button.preset")].filter((b) => b.getAttribute("aria-pressed") === "true").map((b) => b.dataset.p).join(","));
check((await lit()) === "Street" && near(await speedSens(), 0.6, 1e-6), `Street lit with the device's speed sensitivity (${await lit()}, ${await speedSens()})`);
await setKb({ steerSpeedSensitivity: 0.9 });
await frames(2);
check((await lit()) === "Street" && near(await speedSens(), 0.9, 1e-6), `speed sensitivity change keeps Street lit (${await lit()}, ${await speedSens()})`);
await page.click('button.preset[data-p="Rookie"]');
check((await lit()) === "Rookie", `Rookie lit (${await lit()})`);

check(errors.length === 0, `no page errors (${errors.slice(0, 3).join(" | ")})`);
await browser.close();
console.log(failures ? `${failures} failure(s)` : "all controls settings checks passed");
process.exit(failures ? 1 : 0);
