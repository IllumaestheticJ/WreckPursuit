// Regression check for "police radio callouts aren't working again" (owner, 2026-10-31, v0.14.5).
// Usage: node e2e/radio_callouts.mjs  (needs `npm run wasm` first).
//
// Root cause: AudioSystem.setPaused called speechSynthesis.pause() on every pause (Esc menu,
// career garage, workshop). Chromium's pause() on an idle synth wedges the speech queue: later
// speak() calls stay pending and never start or fail, even after resume(). A career session
// always starts behind the garage (a pause), so no callout was ever spoken there; in the
// sandbox the first Esc killed the voice for the rest of the session. Radio events, the
// director, the scanner and the strip all kept working, which is why nothing logged an error.
//
// Checks, in headless Chromium (no system voices: an utterance the engine takes up ends in
// "synthesis-failed", which `speech().reached` counts; a wedged one never ends):
// 1. Sandbox night run, paused and resumed (the Esc menu) before the chase: police radio lines
//    are said by the scanner and every spoken one reaches the speech engine.
// 2. Career: behind the garage the context runs with only the Ui bus open (garage sounds) and
//    the Police bus shut; driving out with a Police Scanner fitted, the buses are open again, the
//    context runs, and the chase's callouts are said and reach the speech engine.
// 3. No page errors.
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5199;
const URL = `http://localhost:${PORT}/`;

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));

async function waitForServer() {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(URL)).ok) return;
    } catch {}
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error("vite did not start");
}

let failures = 0;
const check = (ok, msg) => {
  console.log(`${ok ? "PASS" : "FAIL"}  ${msg}`);
  if (!ok) failures++;
};

await waitForServer();
const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: [
    "--use-gl=angle",
    "--use-angle=swiftshader",
    "--enable-unsafe-swiftshader",
    "--ignore-gpu-blocklist",
    "--autoplay-policy=no-user-gesture-required",
  ],
});
const page = await (await browser.newContext({ viewport: { width: 1400, height: 860 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));

await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });

const state = () =>
  page.evaluate(() => {
    const a = window.__wpAudio;
    const gates = a.mixer ? Object.fromEntries(Object.entries(a.mixer.buses).map(([k, b]) => [k, b.gate.gain.value])) : {};
    return {
      ctx: a.ctx?.state ?? "none",
      uiOnly: a.uiOnly,
      hears: a.hearsRadio(),
      said: a.radioLog().length,
      speech: a.speech(),
      gates,
      synth: { speaking: speechSynthesis.speaking, pending: speechSynthesis.pending, paused: speechSynthesis.paused },
    };
  });

/** Wait until the scanner has said a line and the engine took up every spoken one. */
const waitForCallouts = (said0, reached0) =>
  page
    .waitForFunction(
      ([s0, r0]) => {
        const a = window.__wpAudio;
        const sp = a.speech();
        return a.radioLog().length > s0 && sp.reached > r0 && sp.reached >= sp.attempts - (speechSynthesis.speaking ? 1 : 0);
      },
      [said0, reached0],
      { timeout: 150000, polling: 500 },
    )
    .catch(() => {});

// 1. Sandbox night run with an Esc pause first.
await page.evaluate(() => {
  window.__wpAudio.setTier("Elite");
  window.__wp.startNightRun("debug_sedan", "Arcade");
});
await page.mouse.click(700, 400); // the first click starts the AudioContext
await page.waitForFunction(() => window.__wpAudio?.running(), null, { timeout: 10000 }).catch(() => {});
await page.evaluate(() => (window.__wp.app.paused = true));
await page.waitForTimeout(800);
await page.evaluate(() => (window.__wp.app.paused = false));
await page.waitForTimeout(300);
await page.evaluate(() => window.__wp.sim.debugSetHeat(window.__wp.app.player, 45));
await page.keyboard.down("KeyW");
await waitForCallouts(0, 0);
const a = await state();
console.log(`      sandbox: ${JSON.stringify(a)}`);
check(a.ctx === "running" && a.hears, `sandbox: the scanner hears the radio after a pause (ctx ${a.ctx})`);
check(a.said > 0, `sandbox: the scanner said police lines (${a.said})`);
if (a.speech.supported) {
  check(a.speech.attempts > 0 && a.speech.reached > 0, `sandbox: spoken callouts reach the speech engine after a pause (attempts ${a.speech.attempts}, reached ${a.speech.reached})`);
}
await page.keyboard.up("KeyW");

// 2. Career: garage (Ui bus only), then a chase with a Police Scanner fitted.
await page.evaluate(() => {
  const s = window.__wp.sim;
  let v = s.careerNewWithCash("Arcade", 80000);
  const d = [...v.dealer].sort((x, y) => x.price_worn - y.price_worn)[0];
  v = s.careerBuy(d.def_id, "Worn");
  s.careerInstall(v.cars[v.cars.length - 1].uid, "elec_scanner_1");
  window.__wp.openGarage();
});
await page.waitForTimeout(1500);
const g = await state();
console.log(`      garage: ${JSON.stringify(g)}`);
check(g.ctx === "running" && g.uiOnly && g.gates.Ui > 0.9 && g.gates.Police < 0.1, `garage: context runs, Ui bus open, Police bus shut (${JSON.stringify(g.gates)})`);
const before = g.speech;
const said0 = g.said;
await page.evaluate(() => window.__wp.startCareerRun("chase", false, true, null));
await page.waitForTimeout(500);
await page.evaluate(() => window.__wp.sim.debugSetHeat(window.__wp.app.player, 45));
await page.keyboard.down("KeyW");
await waitForCallouts(said0, before.reached);
await page.keyboard.up("KeyW");
const c = await state();
console.log(`      career: ${JSON.stringify(c)}`);
check(
  c.ctx === "running" && !c.uiOnly && Object.values(c.gates).every((v) => v > 0.9),
  `career run: context running and every bus open again after the garage (${JSON.stringify(c.gates)})`,
);
check(c.hears && c.said > said0, `career run: the scanner said police lines (${c.said - said0} new)`);
if (c.speech.supported) {
  check(
    c.speech.attempts > before.attempts && c.speech.reached > before.reached,
    `career run: callouts reach the speech engine after the garage (attempts ${c.speech.attempts - before.attempts}, reached ${c.speech.reached - before.reached})`,
  );
}

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 5).join(" | ") : ""}`);
await browser.close();
console.log(failures ? `${failures} FAILED` : "radio callouts e2e: all passed");
process.exit(failures ? 1 : 0);
