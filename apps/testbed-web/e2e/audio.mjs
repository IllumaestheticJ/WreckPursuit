// End-to-end check of the audio backend (workstream 13) in headless Chromium.
// Usage: node e2e/audio.mjs  (needs the wasm package built first, like e2e/run.mjs).
//
// Checks: the AudioContext starts on a key press, recorded sample files all decode, driving
// plays engine + surface voices, a
// forced siren plays, the head-on crash scenario produces impact sounds, a scanner line is
// spoken after the tier delay, voice counts stay within the configured limits, no page errors.
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5198;
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
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
const counts = () => page.evaluate(() => ({ ...window.__wpAudio.cueCounts }));
const stats = () => page.evaluate(() => window.__wpAudio.stats());
const sum = (c, prefix) => Object.entries(c).filter(([k]) => k.startsWith(prefix)).reduce((a, [, v]) => a + v, 0);

// 1. Start + drive.
await page.keyboard.press("KeyX");
await page.waitForFunction(() => window.__wpAudio?.running(), null, { timeout: 10000 });
check(true, "AudioContext running after a key press");
await page
  .waitForFunction(() => window.__wpAudio.samples()?.loading === 0, null, { timeout: 30000 })
  .catch(() => {});
const smp = await page.evaluate(() => window.__wpAudio.samples());
check(smp && smp.ready > 0 && smp.failed === 0 && smp.loading === 0, `recorded samples decoded (${JSON.stringify(smp)})`);
await page.keyboard.down("KeyW");
await page.waitForTimeout(3000);
await page.keyboard.up("KeyW");
let c = await counts();
check(sum(c, "engine/") > 0, `engine layers played (${sum(c, "engine/")})`);
check(sum(c, "surface/") > 0, `surface rolling noise played (${sum(c, "surface/")})`);
let st = await stats();
check(st && st.looped > 0, `director has looped voices (${st?.looped})`);

// 2. Siren on the player car.
await page.evaluate(() => window.__wpAudio.setSiren("Yelp"));
await page.waitForTimeout(500);
c = await counts();
check((c["siren/yelp"] ?? 0) > 0, "forced yelp siren plays");
await page.evaluate(() => window.__wpAudio.setSiren("Default"));

// 3. Scanner line (Elite = 3 s delay).
await page.evaluate(() => {
  window.__wpAudio.setTier("Elite");
  window.__wpAudio.testRadio("Pursuit");
});
const t0 = await page.evaluate(() => window.__wp.sim.worldInfo()[0]);
await page
  .waitForFunction(() => window.__wpAudio.radioLog().length > 0, null, { timeout: 30000 })
  .catch(() => {});
const log = await page.evaluate(() => window.__wpAudio.radioLog());
const t1 = await page.evaluate(() => window.__wp.sim.worldInfo()[0]);
console.log(`      scanner: ${((t1 - t0) / 240).toFixed(2)} s of sim time, queue ${JSON.stringify(await stats())}`);
check(log.length > 0, `scanner spoke a line after the tier delay (${log.length})`);

// 4. Crash: impacts from real contacts.
const before = sum(await counts(), "impact/");
await page.evaluate(() => window.__wp.runScenario("crash/head_on.ron", 50));
await page.waitForFunction(() => window.__wp.sim.scenarioResult()?.done === true, null, { timeout: 60000 });
c = await counts();
check(sum(c, "impact/") > before, `head-on crash played impact sounds (${sum(c, "impact/") - before})`);
const rec = await page.evaluate(() => window.__wpAudio.samples());
check(rec.recorded > 0 && rec.fallback === 0, `crash used recorded samples (${rec.recorded} recorded, ${rec.fallback} fallback)`);
st = await stats();
check(st.looped + st.one_shots <= 40, `voice count within max_voices (${st.looped}+${st.one_shots})`);
console.log("cue counts:", JSON.stringify(c));

// 5. Cost with 24 cars (informational; recorded in docs/integration/audio.md).
await page.goto(URL + "?bench=24");
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
await page.keyboard.press("KeyX");
await page.waitForFunction(() => window.__wpAudio?.running(), null, { timeout: 10000 });
const plays0 = Object.values(await counts()).reduce((a, b) => a + b, 0);
const frames0 = await page.evaluate(() => window.__wpAudio.frames);
await page.waitForTimeout(6000);
const plays = Object.values(await counts()).reduce((a, b) => a + b, 0) - plays0;
const fps = ((await page.evaluate(() => window.__wpAudio.frames)) - frames0) / 6;
console.log(`      24 cars: ${plays} plays in 6 s at ${fps.toFixed(1)} fps (headless, software GL)`);
const perf = await page.evaluate(() => ({ ms: window.__wpAudio.frameMs, dir: window.__wpAudio.directorMs, app: window.__wpAudio.applyMs, st: window.__wpAudio.stats() }));
console.log(`      24 cars: audio frame ${perf.ms.toFixed(3)} ms (director ${perf.dir.toFixed(3)} ms, apply ${perf.app.toFixed(3)} ms), voices ${perf.st.looped}+${perf.st.one_shots}, culled ${perf.st.culled}`);
check(perf.st.looped + perf.st.one_shots <= 40, "24-car bench stays within max_voices");

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 5).join(" | ") : ""}`);
await browser.close();
process.exit(failures ? 1 : 0);
