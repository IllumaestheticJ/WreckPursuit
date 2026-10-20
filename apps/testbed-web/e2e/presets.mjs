// One-click assist presets (Rookie / Street / Sim, GDD §6) in headless Chromium (Playwright).
// Usage: npm run e2e:presets  (needs `npm run wasm` first). Env: CHROMIUM=/path/to/chrome,
// SHOTS=dir to write a screenshot of the preset row.
//
// Checks: each preset button sets damage mode + assists in one click and lights up; ticking a
// box afterwards shows "custom"; the chosen preset (and its mode) survives a reload.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5197;
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

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const context = await browser.newContext({ viewport: { width: 1280, height: 720 } });
const page = await context.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
const ready = () => page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });

const state = () =>
  page.evaluate(() => {
    const boxes = {};
    document.querySelectorAll("#assist-row input[data-a]").forEach((cb) => (boxes[cb.dataset.a] = cb.checked));
    document.querySelectorAll("#assist-row select.assist-level").forEach((sel) => (boxes[sel.dataset.a] = sel.value));
    const hud = document.querySelector(".hud-main")?.textContent ?? "";
    const pressed = [...document.querySelectorAll("button.preset")]
      .filter((b) => b.getAttribute("aria-pressed") === "true")
      .map((b) => b.dataset.p);
    return {
      mode: document.getElementById("mode").value,
      boxes,
      pressed,
      label: document.getElementById("preset-state").textContent,
      hud,
      sim: window.__wp.sim.assists(window.__wp.app.player),
    };
  });

await page.goto(URL);
await ready();
let s0;

const expected = {
  Rookie: { mode: "Arcade", abs: "Full", tcs: "Full", esc: true, auto_gears: true },
  Street: { mode: "Arcade", abs: "Full", tcs: "Half", esc: false },
  Sim: { mode: "Simulation", abs: "Half", tcs: "Off", esc: false, auto_gears: false },
};
for (const [name, want] of Object.entries(expected)) {
  await page.click(`button.preset[data-p="${name}"]`);
  const s = await state();
  const boxesOk = Object.entries(want)
    .filter(([k]) => k !== "mode")
    .every(([k, v]) => s.boxes[k] === v);
  check(s.mode === want.mode && boxesOk, `${name}: mode ${s.mode}, boxes ${JSON.stringify(s.boxes)}`);
  check(s.pressed.length === 1 && s.pressed[0] === name && s.label === "", `${name} button lit (${s.pressed})`);
}

// TCS / ABS selectors (Off / Half / Full): a level reaches the sim, the HUD tag shows it, custom.
await page.click(`button.preset[data-p="Street"]`);
await page.selectOption('#assist-row select[data-a="abs"]', "Half");
await page.waitForTimeout(100);
s0 = await state();
check(s0.sim.abs === "Half" && s0.sim.tcs === "Half" && s0.label === "custom", `ABS Half picked: sim ${s0.sim.abs}/${s0.sim.tcs}, ${s0.label}`);
check(/ABS HALF/.test(s0.hud) && /TCS HALF/.test(s0.hud), `HUD tags show the levels (${s0.hud.match(/ABS[^A-Z]*\w*|TCS[^A-Z]*\w*/g)})`);
const ov = await page.evaluate(() => JSON.parse(localStorage.getItem("wp.assistOverrides") ?? "{}"));
check(ov.abs === "Half" && ov.tcs === "Half", `levels remembered (${JSON.stringify(ov)})`);
// A legacy bool in stored overrides still loads (true → Full).
await page.evaluate(() => {
  localStorage.setItem("wp.assistPreset", "");
  localStorage.removeItem("wp.gameSettings.v1"); // else the menu's stored levels win on load
  localStorage.setItem("wp.assistOverrides", JSON.stringify({ abs: false, tcs: true }));
});
await page.reload();
await ready();
s0 = await state();
check(s0.sim.abs === "Off" && s0.sim.tcs === "Full" && s0.boxes.tcs === "Full", `legacy bool overrides load as levels (${s0.sim.abs}/${s0.sim.tcs})`);

// Street, then tick ESC by hand → custom.
await page.click(`button.preset[data-p="Street"]`);
await page.click(`#assist-row input[data-a="esc"]`);
let s = await state();
check(s.pressed.length === 0 && s.label === "custom" && s.boxes.esc, `hand-ticked ESC → custom (${s.label})`);

// Preset survives a reload, with its damage mode.
await page.click(`button.preset[data-p="Sim"]`);
if (shots) await page.locator("#preset-row").screenshot({ path: join(shots, "presets.png") });
await page.reload();
await ready();
s = await state();
check(s.mode === "Simulation" && s.pressed[0] === "Sim" && s.boxes.abs === "Half" && s.boxes.tcs === "Off", `Sim remembered after reload (${s.mode}, ${s.pressed})`);

check(errors.length === 0, `no page errors (${errors.slice(0, 3).join(" | ")})`);
await browser.close();
console.log(failures ? `${failures} failure(s)` : "all preset checks passed");
process.exit(failures ? 1 : 0);
