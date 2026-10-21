// End-to-end check of the recorded sample packs (workstream 13, ws/audio-samples).
// Usage: node e2e/audio_samples.mjs  (needs the wasm package built first, like e2e/run.mjs).
//
// Checks: every recorded file referenced by the packs is fetched from a relative URL under
// data/audio/packs/ (the published link serves the same layout), decodes without errors,
// driving plays recorded engine packs, switching to the procedural fallback and back works,
// and no page errors occur.
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
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--autoplay-policy=no-user-gesture-required"],
});
const page = await (await browser.newContext({ viewport: { width: 1200, height: 760 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => (m.type() === "error" || m.text().startsWith("audio sample failed")) && errors.push(m.text()));
const audioRequests = [];
page.on("request", (r) => /\.(ogg|mp3|wav)$/.test(r.url()) && audioRequests.push(r.url()));
const bad = [];
page.on("response", (r) => /\.(ogg|mp3|wav)$/.test(r.url()) && !r.ok() && bad.push(`${r.status()} ${r.url()}`));

await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });
await page.keyboard.press("KeyX");
await page.waitForFunction(() => window.__wpAudio?.running(), null, { timeout: 10000 });
await page.waitForFunction(() => window.__wpAudio.sampleStatus()?.loading === 0, null, { timeout: 30000 });
const s = await page.evaluate(() => window.__wpAudio.sampleStatus());
check(s.total > 0 && s.loaded === s.total, `all recorded samples decoded (${s.loaded}/${s.total})`);
check(s.failed.length === 0, `no sample failed to load${s.failed.length ? ": " + s.failed.join(", ") : ""}`);
check(bad.length === 0, `no HTTP errors for audio files${bad.length ? ": " + bad.slice(0, 5).join(", ") : ""}`);
const base = new globalThis.URL("data/audio/packs/", URL).href;
check(
  audioRequests.length === s.total && audioRequests.every((u) => u.startsWith(base) && !u.includes("/../")),
  `${audioRequests.length} audio requests, all relative under data/audio/packs/`,
);
check((await page.evaluate(() => window.__wpAudio.soundSource())) === "recorded", "recorded samples are the default source");

// Drive: recorded engine layers and the ambience play. The player's engine is the modelled
// synthesizer by default (row 73); layers are selected here so the recordings are exercised.
await page.evaluate(() => window.__wpAudio.setEngineVoicing("layers"));
const engineRecPlays = () =>
  page.evaluate(() =>
    Object.entries(window.__wpAudio.cueCounts)
      .filter(([k]) => k.startsWith("engine/") && k !== "engine/physical")
      .reduce((a, [, v]) => a + v, 0),
  );
await page.keyboard.down("KeyW");
await page.waitForTimeout(2500);
await page.keyboard.up("KeyW");
check((await engineRecPlays()) > 0, "engine layers played while driving");

// Fallback and back.
await page.evaluate(() => window.__wpAudio.setSource("procedural"));
await page.waitForTimeout(800);
check((await page.evaluate(() => window.__wpAudio.soundSource())) === "procedural", "switched to the procedural fallback");
await page.evaluate(() => window.__wpAudio.setSource("recorded"));
await page.waitForTimeout(800);
check((await page.evaluate(() => window.__wpAudio.soundSource())) === "recorded", "switched back to recorded samples");
const st = await page.evaluate(() => window.__wpAudio.stats());
check(st && st.looped > 0, `director re-played its loops after switching (${st?.looped})`);
// Playtest #47: the reset's Stop commands were dropped, so a source switch left the old loops
// playing next to the re-played ones. Toggle a few times while driving, then compare.
await page.keyboard.down("KeyW");
for (const src of ["procedural", "recorded", "procedural", "recorded"]) {
  await page.evaluate((s) => window.__wpAudio.setSource(s), src);
  await page.waitForTimeout(400);
}
await page.keyboard.up("KeyW");
await page.waitForTimeout(300);
const lc = await page.evaluate(() => window.__wpAudio.loopCounts());
check(lc && lc.backend === lc.director, `no stale loops after source switches (backend ${lc?.backend}, director ${lc?.director})`);
await page.evaluate(() => window.__wpAudio.setEngineVoicing("modelled"));

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 5).join(" | ") : ""}`);
await browser.close();
server.kill("SIGTERM");
console.log(failures ? `${failures} FAILED` : "all passed");
process.exit(failures ? 1 : 0);
