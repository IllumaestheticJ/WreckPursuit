// Gamepad + rebindable controls check (headless Chromium, Playwright). Run by `npm run e2e`
// (run.mjs) at its end, or alone: `npm run e2e:gamepad` (needs `npm run wasm` first).
// Env: CHROMIUM=/path/to/chrome, GAMEPAD_SHOTS=dir to write controls_panel.png.
//
// A fake standard-mapping gamepad replaces `navigator.getGamepads`. Checks: the pad drives
// forward and steers with its analogue values passed straight through (deadzone only, no ramps);
// the source and the sim's AssistProfile switch to the gamepad preset and back; pad buttons fire
// edge actions (camera cycle); a disconnect mid-drive releases everything; the controls panel
// blocks driving input, rebinds a key and a pad button, and the rebind survives a reload (and
// shows in the F1 help); reset to defaults; input latency (pad change → the sim's applied input)
// stays within one frame. Clutch (playtest row 59): an old save without it migrates; bound to a
// key and a wheel-style pedal axis, survives a reload; with clutch: manual, holding it gives
// clt 1 and the engine revs while the car does not move, releasing it drives off.
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5195;
const shots = process.env.GAMEPAD_SHOTS;
const URL = `http://localhost:${PORT}/`;

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

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const context = await browser.newContext({ viewport: { width: 1400, height: 860 } });
// The fake pad: `window.__pad.set({ axes: {0: 0.5}, buttons: {7: 1} })`, `.connect(bool)`.
await context.addInitScript(() => {
  const pad = {
    id: "WreckPursuit fake pad (STANDARD GAMEPAD)",
    index: 0,
    connected: false,
    mapping: "standard",
    timestamp: 0,
    // Axes 0–3 sticks; axis 5 a wheel-style clutch pedal resting at +1 (pressed = -1).
    axes: [0, 0, 0, 0, 0, 1],
    buttons: Array.from({ length: 17 }, () => ({ pressed: false, touched: false, value: 0 })),
  };
  Object.defineProperty(Navigator.prototype, "getGamepads", {
    configurable: true,
    value: () => [pad.connected ? pad : null, null, null, null],
  });
  window.__pad = {
    raw: pad,
    set({ axes = {}, buttons = {} } = {}) {
      for (const [i, v] of Object.entries(axes)) pad.axes[i] = v;
      for (const [i, v] of Object.entries(buttons)) pad.buttons[i] = { pressed: v > 0.5, touched: v > 0, value: v };
      pad.timestamp++;
    },
    clear() {
      pad.axes.fill(0);
      pad.axes[5] = 1;
      for (let i = 0; i < pad.buttons.length; i++) pad.buttons[i] = { pressed: false, touched: false, value: 0 };
      pad.timestamp++;
    },
    connect(on) {
      pad.connected = on;
      const ev = new Event(on ? "gamepadconnected" : "gamepaddisconnected");
      ev.gamepad = pad;
      window.dispatchEvent(ev);
    },
  };
});
const page = await context.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));

const ready = () => page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
const speed = () => page.evaluate(() => window.__wp.sim.states()[14]);
const frames = (n) => page.evaluate((k) => new Promise((r) => { let i = 0; const f = () => (++i >= k ? r() : requestAnimationFrame(f)); requestAnimationFrame(f); }), n);
// Telemetry channels 9..11: the sim's applied (post-assist) throttle, brake, steer.
const applied = () => page.evaluate(() => {
  const t = window.__wp.sim.telemetry(window.__wp.app.player);
  return { throttle: t[9], brake: t[10], steer: t[11] };
});
// Assist fields that differ between the keyboard and gamepad presets and are not owner overrides.
const presetDiff = () => page.evaluate(() => {
  const k = window.__wp.sim.assistPreset("Keyboard");
  const g = window.__wp.sim.assistPreset("Gamepad");
  const cur = window.__wp.sim.assists(window.__wp.app.player);
  const keys = Object.keys(g).filter((f) => !["abs", "tcs", "esc", "auto_gears"].includes(f) && JSON.stringify(g[f]) !== JSON.stringify(k[f]));
  const is = (p) => keys.every((f) => JSON.stringify(cur[f]) === JSON.stringify(p[f]));
  return { keys, gamepad: is(g), keyboard: is(k) };
});

// Start from a clean binding set.
await page.goto(URL);
await ready();
await page.evaluate(() => localStorage.removeItem("wp.bindings.v1"));
await page.reload();
await ready();

// 1. Connect and drive: RT full, left stick 60 % right.
await page.evaluate(() => {
  window.__pad.connect(true);
  window.__pad.set({ axes: { 0: 0.6 }, buttons: { 7: 1 } });
});
await page.waitForTimeout(2500);
const v = await speed();
const a1 = await applied();
const src = await page.evaluate(() => window.__wp.app.source);
check(v > 1.5 && a1.steer > 0.2, `fake gamepad drives forward and steers (speed ${v.toFixed(2)} m/s, applied steer ${a1.steer.toFixed(2)})`);
const d1 = await presetDiff();
check(src === "Gamepad" && d1.gamepad && d1.keys.length > 0, `source → Gamepad and the sim uses the gamepad assist preset (fields ${d1.keys.join(", ")})`);

// 2. Analogue values reach the sim input as read: deadzone + curve only, no ramps.
await page.evaluate(() => window.__pad.set({ axes: { 0: 0.5 }, buttons: { 7: 0.4 } }));
await frames(2);
const raw = await page.evaluate(() => ({ ...window.__wp.app.input }));
const wantSteer = (0.5 - 0.12) / (1 - 0.12);
const wantThr = (0.4 - 0.05) / (1 - 0.05);
check(Math.abs(raw.steer - wantSteer) < 1e-3 && Math.abs(raw.throttle - wantThr) < 1e-3, `analogue pass-through (steer ${raw.steer.toFixed(3)} want ${wantSteer.toFixed(3)}, throttle ${raw.throttle.toFixed(3)} want ${wantThr.toFixed(3)})`);

// 3. Latency: a pad change shows in the sim's applied input on the next frame.
await page.evaluate(() => window.__pad.set({ axes: { 0: 0 }, buttons: { 7: 0, 6: 0 } }));
await page.waitForFunction(() => window.__wp.sim.telemetry(window.__wp.app.player)[9] === 0, null, { timeout: 5000 });
const lat = await page.evaluate(
  () =>
    new Promise((resolve) => {
      // Set the pad between two frames, then count frame callbacks (ours run after main's).
      requestAnimationFrame(() => {
        window.__pad.set({ buttons: { 7: 1 } });
        let n = 0;
        const out = { input: -1, sim: -1 };
        const f = () => {
          n++;
          if (out.input < 0 && window.__wp.app.input.throttle > 0) out.input = n;
          if (out.sim < 0 && window.__wp.sim.telemetry(window.__wp.app.player)[9] > 0) out.sim = n;
          if ((out.input > 0 && out.sim > 0) || n > 30) resolve(out);
          else requestAnimationFrame(f);
        };
        requestAnimationFrame(f);
      });
    }),
);
check(lat.input === 1 && lat.sim === 1, `input latency one frame (pad → input: ${lat.input} frame(s), pad → sim applied throttle: ${lat.sim} frame(s))`);

// 4. Edge action from a pad button: View cycles the camera once per press.
const cam0 = await page.evaluate(() => window.__wp.rig.mode);
await page.evaluate(() => window.__pad.set({ buttons: { 8: 1 } }));
await frames(3);
await page.evaluate(() => window.__pad.set({ buttons: { 8: 0 } }));
await frames(2);
const cam1 = await page.evaluate(() => window.__wp.rig.mode);
check(cam1 !== cam0, `pad View button cycles the camera (${cam0} → ${cam1})`);
// R3 held looks back.
await page.evaluate(() => window.__pad.set({ buttons: { 11: 1 } }));
await frames(2);
const lb = await page.evaluate(() => window.__wp.rig.lookBack);
await page.evaluate(() => window.__pad.set({ buttons: { 11: 0 } }));
await frames(2);
const lbOff = await page.evaluate(() => window.__wp.rig.lookBack);
check(lb && !lbOff, `pad R3 held looks back, released looks forward`);

// 5. Disconnect mid-drive with throttle + steer held: everything releases, keyboard preset.
await page.evaluate(() => window.__pad.set({ axes: { 0: -1 }, buttons: { 7: 1, 0: 1 } }));
await frames(3);
await page.evaluate(() => window.__pad.connect(false));
await frames(3);
const after = await page.evaluate(() => ({ ...window.__wp.app.input, source: window.__wp.app.source }));
const d2 = await presetDiff();
check(
  after.throttle === 0 && after.steer === 0 && after.handbrake === 0 && after.source === "Keyboard" && d2.keyboard,
  `pad disconnect mid-drive releases everything and returns to the keyboard preset (${JSON.stringify({ t: after.throttle, s: after.steer, hb: after.handbrake, src: after.source })})`,
);
await page.evaluate(() => {
  window.__pad.clear();
  window.__pad.connect(true);
});

// 6. Controls panel: blocks driving input; rebind throttle key slot 1 to Backslash and pad horn
// slot 1 to B (button 1).
await page.keyboard.press("F9");
check(await page.evaluate(() => window.__wp.controls.isOpen), "F9 opens the controls panel");
await page.keyboard.down("KeyW");
await frames(4);
const blocked = await page.evaluate(() => window.__wp.app.input.throttle);
await page.keyboard.up("KeyW");
check(blocked === 0, `driving input is neutral while the panel is open (throttle ${blocked})`);
await page.click('button.bind[data-d="keys"][data-a="throttle"][data-s="0"]');
await page.keyboard.press("Backslash");
if (shots) await page.screenshot({ path: join(shots, "controls_panel.png") });
await page.click('button.bind[data-d="pad"][data-a="horn"][data-s="0"]');
await frames(2);
await page.evaluate(() => window.__pad.set({ buttons: { 1: 1 } }));
await frames(3);
await page.evaluate(() => window.__pad.set({ buttons: { 1: 0 } }));
const saved = await page.evaluate(() => JSON.parse(localStorage.getItem("wp.bindings.v1") ?? "null"));
check(
  saved?.keys?.throttle?.[0] === "Backslash" && saved?.pad?.horn?.[0]?.button === 1 && !saved?.pad?.reset?.some?.((c) => c.button === 1),
  `rebind saved to wp.bindings.v1 (throttle ${JSON.stringify(saved?.keys?.throttle)}, horn ${JSON.stringify(saved?.pad?.horn)})`,
);
await page.keyboard.press("Escape");
check(!(await page.evaluate(() => window.__wp.controls.isOpen)), "Esc closes the panel");

// 7. Survives a reload: the new key drives, the old one does not, the help lists it.
await page.reload();
await ready();
const b = await page.evaluate(() => window.__wp.bindings());
check(b.keys.throttle[0] === "Backslash" && b.pad.horn[0].button === 1, `rebind survives a reload (throttle ${b.keys.throttle.join(", ")})`);
await page.keyboard.down("KeyW");
await frames(5);
const oldKey = await page.evaluate(() => window.__wp.app.input.throttle);
await page.keyboard.up("KeyW");
await page.keyboard.down("Backslash");
await page.waitForTimeout(2000);
const v2 = await speed();
await page.keyboard.up("Backslash");
check(oldKey === 0 && v2 > 1.0, `rebound key drives (speed ${v2.toFixed(2)} m/s), the old key no longer does`);
await page.keyboard.press("F1");
const help = await page.evaluate(() => document.querySelector(".help-binds")?.textContent ?? "");
check(help.includes("throttle — \\") && help.includes("horn — Q · pad B"), `F1 help shows the current bindings`);
await page.keyboard.press("F1");

// 8. Reset to defaults.
await page.evaluate(() => window.__wp.controls.open());
await page.click("#bind-reset");
const reset = await page.evaluate(() => ({ b: window.__wp.bindings(), stored: localStorage.getItem("wp.bindings.v1") }));
await page.evaluate(() => window.__wp.controls.close());
check(reset.b.keys.throttle[0] === "KeyW" && reset.stored === null, "reset to defaults clears the saved bindings");

// 9. Clutch. An older save (no clutch entry) migrates: its bindings stay, the clutch gets defaults.
await page.evaluate(() => {
  const old = { keys: { horn: ["KeyQ"] }, pad: { horn: [{ button: 1 }] }, padDeadzone: 0.1, padCurve: 1.2 };
  localStorage.setItem("wp.bindings.v1", JSON.stringify(old));
});
await page.reload();
await ready();
const mig = await page.evaluate(() => window.__wp.bindings());
check(
  Array.isArray(mig.keys.clutch) && mig.keys.clutch.length === 0 && mig.pad.horn[0].button === 1 && mig.padCurve === 1.2 && mig.keys.throttle[0] === "KeyW",
  "an older wp.bindings.v1 save (no clutch) loads: its bindings kept, clutch unbound by default",
);
await page.evaluate(() => {
  window.__pad.clear();
  window.__pad.connect(true);
});
await page.keyboard.press("F9");
await page.click('button.bind[data-d="keys"][data-a="clutch"][data-s="0"]');
await page.keyboard.press("Backslash");
await page.click('button.bind[data-d="pad"][data-a="clutch"][data-s="0"]');
await frames(2);
await page.evaluate(() => window.__pad.set({ axes: { 5: -1 } }));
await frames(3);
await page.evaluate(() => window.__pad.set({ axes: { 5: 1 } }));
await page.keyboard.press("Escape");
await page.selectOption("#clutch-mode", "manual");
await page.reload();
await ready();
const cb = await page.evaluate(() => ({
  b: window.__wp.bindings(),
  mode: document.querySelector("#clutch-mode").value,
  auto: window.__wp.sim.assists(window.__wp.app.player).auto_clutch,
}));
const pedal = cb.b.pad.clutch[0];
check(
  cb.b.keys.clutch[0] === "Backslash" && pedal?.axis === 5 && pedal?.dir === -1 && pedal?.full === true,
  `clutch bound to a key and a pedal axis, survives a reload (${JSON.stringify(cb.b.keys.clutch)}, ${JSON.stringify(cb.b.pad.clutch)})`,
);
check(cb.mode === "manual" && cb.auto === false, `clutch: manual survives a reload and the sim's auto_clutch is off (${cb.mode}, auto_clutch ${cb.auto})`);
// Pedal half way → clutch 0.5 on the input (full-range axis).
await page.evaluate(() => {
  window.__pad.connect(true);
  window.__pad.set({ axes: { 5: 0 } });
});
await frames(3);
const half = await page.evaluate(() => window.__wp.app.input.clutch);
await page.evaluate(() => window.__pad.set({ axes: { 5: 1 } }));
await frames(2);
await page.evaluate(() => window.__pad.connect(false));
check(Math.abs(half - 0.5) < 0.06, `wheel-style pedal axis: half travel → clutch ${half.toFixed(2)}`);
// Standstill, clutch held, full throttle: the engine revs, the car stays put.
await page.evaluate(() => window.__wp.sim.reset(window.__wp.app.player));
await frames(10);
// Clutch in first (as a driver would), then full throttle.
await page.keyboard.down("Backslash");
await page.waitForTimeout(400);
const v0 = await speed();
await page.keyboard.down("KeyW");
await page.waitForTimeout(1500);
const held = await page.evaluate(() => ({
  clt: window.__wp.app.input.clutch,
  rpm: window.__wp.sim.telemetry(window.__wp.app.player)[1],
  v: window.__wp.sim.states()[14],
}));
await page.keyboard.up("Backslash");
await page.waitForTimeout(2000);
const released = await speed();
await page.keyboard.up("KeyW");
check(
  held.clt === 1 && held.v - v0 < 0.3 && held.rpm > 2500,
  `manual clutch held: clt ${held.clt}, engine ${held.rpm.toFixed(0)} rpm, car ${v0.toFixed(2)} → ${held.v.toFixed(2)} m/s over 1.5 s of full throttle (drive decoupled)`,
);
check(released > 2, `clutch released with throttle on: the car drives off (${released.toFixed(2)} m/s)`);
await page.selectOption("#clutch-mode", "auto");
const back = await page.evaluate(() => window.__wp.sim.assists(window.__wp.app.player).auto_clutch);
check(back === true, "clutch: automatic restores the auto clutch");
await page.evaluate(() => localStorage.removeItem("wp.bindings.v1"));

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
console.log(failures ? `${failures} gamepad check(s) failed` : "all gamepad checks passed");
process.exit(failures ? 1 : 0);
