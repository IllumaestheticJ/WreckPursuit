// Instant crash replay (lead, row 70) in headless Chromium. Usage: node e2e/crash_replay.mjs
// (needs `npm run wasm`). Env: CHROMIUM, CRASH_SHOTS=dir for screenshots.
//
// Checks: a scripted crash (56 km/h into a barrier) triggers the offer; accepting it (Enter)
// plays the replay (letterbox, game UI hidden, 0.25× around the impact, tick hashes verified
// against the live log) and returns to live play with the same tick and state hash as before;
// the scripted scenario then still ends on simcli's final hash. In a city free drive with
// "auto crash replay" on, knocking a parked car plays the replay without asking and the
// action key skips it; benches never offer one.
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5194;
const URL = `http://localhost:${PORT}/`;
const shots = process.env.CRASH_SHOTS;
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
  console.log(`${ok ? "PASS" : "FAIL"}  crash replay: ${msg}`);
  if (!ok) failures++;
};

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1280, height: 720 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
const shot = async (name) => shots && page.screenshot({ path: join(shots, name) });
const phase = () => page.evaluate(() => window.__wp.crash.phase);

await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });

// 1. Scripted crash → offer.
await page.evaluate(() => window.__wp.setAutoCrashReplay(false));
await page.evaluate(() => window.__wp.runScenario("crash/barrier_frontal.ron", 56));
await page.waitForFunction(() => window.__wp.crash.phase === "offer", null, { timeout: 60000 }).catch(() => {});
const offer = await page.evaluate(() => ({
  phase: window.__wp.crash.phase,
  mark: window.__wp.crash.mark,
  visible: getComputedStyle(document.getElementById("crash-offer")).display !== "none",
  text: document.getElementById("crash-offer").textContent,
  buffer: window.__wp.sim.crashBufferTicks(),
}));
check(offer.phase === "offer" && offer.visible, `56 km/h into the barrier offers a replay ("${offer.text}")`);
check(
  offer.mark && offer.mark.side === "front" && offer.mark.other === null && offer.mark.closing_mps > 10,
  `crash: ${offer.mark?.side} hit, closing ${offer.mark?.closing_mps?.toFixed(1)} m/s, Δv ${offer.mark?.delta_v_mps?.toFixed(1)} m/s, buffer ${offer.buffer.join(",")}`,
);
await shot("crash_offer.png");

// 2. Accept with the bound key (Enter by default) → replay.
await page.keyboard.press("Enter");
await page.waitForTimeout(100);
const playing = await page.evaluate(() => ({
  phase: window.__wp.crash.phase,
  active: window.__wp.sim.crashReplayActive(),
  info: window.__wp.crash.info,
  letterbox: document.getElementById("crash-letterbox").classList.contains("on"),
  overlay: document.getElementById("overlay").style.visibility,
}));
check(playing.phase === "playing" && playing.active, "Enter plays the replay (live session parked)");
check(playing.letterbox && playing.overlay === "hidden", "letterbox on, game UI hidden");
check(
  playing.info && playing.info.end_tick - playing.info.crash.tick === 480 && playing.info.crash.tick - playing.info.start_tick === 720,
  `window impact −3 s … +2 s (ticks ${playing.info?.start_tick}–${playing.info?.end_tick}, impact ${playing.info?.crash.tick})`,
);
// Sample the playback speed and grab a shot at the impact.
let minSpeed = 9;
let shotTaken = false;
for (let i = 0; i < 400; i++) {
  const s = await page.evaluate(() => ({ st: window.__wp.sim.replayStatus(), phase: window.__wp.crash.phase, info: window.__wp.crash.info }));
  if (s.phase !== "playing" || !s.st) break;
  minSpeed = Math.min(minSpeed, s.st.speed);
  if (!shotTaken && s.info && s.st.tick >= s.info.crash.tick) {
    await shot("crash_replay_impact.png");
    shotTaken = true;
  }
  await page.waitForTimeout(50);
}
check(Math.abs(minSpeed - 0.25) < 0.02, `impact plays at 0.25× (slowest ${minSpeed.toFixed(3)})`);
await page.waitForFunction(() => window.__wp.crash.phase === "idle", null, { timeout: 30000 });
const back = await page.evaluate(() => ({
  last: window.__wp.crash.last,
  active: window.__wp.sim.crashReplayActive(),
  tick: window.__wp.sim.worldInfo()[0],
  overlay: document.getElementById("overlay").style.visibility,
}));
check(!back.active && back.overlay === "", "back to live play, UI visible");
check(
  back.last?.same === true && back.last.drift === 0 && back.last.verified > 0 && !back.last.skipped,
  `live tick/hash unchanged (${back.last?.before.tick} ${back.last?.before.hash} → ${back.last?.after?.tick} ${back.last?.after?.hash}), ` +
    `replay verified ${back.last?.verified} ticks, drift ${back.last?.drift}`,
);
await page.waitForFunction(() => window.__wp.sim.scenarioResult()?.done === true, null, { timeout: 120000 });
const res = await page.evaluate(() => window.__wp.sim.scenarioResult());
check(res.pass && res.matches_simcli === true, `the scenario still ends on simcli's final hash (live = simcli: ${res.matches_simcli})`);

// 3. City free drive, auto replay on: knock a parked car; the replay plays without an offer,
// and the action key skips it.
await page.evaluate(() => {
  window.__wp.setAutoCrashReplay(true);
  window.__wp.startFreeDrive("debug_sedan", "Arcade", true);
});
await page.waitForFunction(() => window.__wp.sim.worldInfo()[0] > 480, null, { timeout: 60000 });
const auto = await page.evaluate(() => {
  const w = window.__wp;
  w.sim.debugKnockProp(w.app.player, -1, 14);
  return w.sim.worldInfo()[0];
});
await page.waitForFunction(() => window.__wp.crash.phase === "playing", null, { timeout: 30000 }).catch(() => {});
const autoPhase = await phase();
check(autoPhase === "playing", `auto crash replay plays a knocked parked car without asking (from tick ${auto})`);
await page.waitForTimeout(1200);
await shot("crash_replay_city.png");
await page.keyboard.press("Enter");
await page.waitForTimeout(100);
const skipped = await page.evaluate(() => ({ phase: window.__wp.crash.phase, last: window.__wp.crash.last }));
check(
  skipped.phase === "idle" && skipped.last?.skipped === true && skipped.last.same === true && skipped.last.drift === 0,
  `skip returns to live play unchanged (${skipped.last?.before.hash} → ${skipped.last?.after?.hash}, drift ${skipped.last?.drift})`,
);
await page.evaluate(() => window.__wp.setAutoCrashReplay(false));

// 4. Snapshot cost in the browser: step a night run by hand (game paused) with the buffer on
// and off; the frames that take a snapshot (every 2 s) show as the spike.
await page.evaluate(() => window.__wp.startNightRun("debug_sedan", "Arcade"));
await page.waitForFunction(() => window.__wp.sim.worldInfo()[0] > 240, null, { timeout: 60000 });
const cost = await page.evaluate(() => {
  const w = window.__wp;
  w.app.paused = true;
  const run = (on) => {
    w.sim.crashDetectEnable(on);
    const t = [];
    for (let i = 0; i < 360; i++) {
      const t0 = performance.now();
      w.sim.step(1 / 60);
      t.push(performance.now() - t0);
    }
    t.sort((a, b) => a - b);
    return { p50: t[180], max: t[t.length - 1], mean: t.reduce((a, b) => a + b, 0) / t.length };
  };
  run(false); // warm up
  const off = run(false);
  const on = run(true);
  w.app.paused = false;
  return { off, on, buffer: w.sim.crashBufferTicks().length };
});
console.log(
  `PERF crash replay buffer (wasm, night run, 60 Hz frames): off p50 ${cost.off.p50.toFixed(2)} / max ${cost.off.max.toFixed(2)} ms, ` +
    `on p50 ${cost.on.p50.toFixed(2)} / max ${cost.on.max.toFixed(2)} ms (snapshot spike ≈ ${(cost.on.max - cost.off.p50).toFixed(1)} ms every 2 s), ` +
    `mean +${(cost.on.mean - cost.off.mean).toFixed(2)} ms/frame`,
);
check(cost.buffer >= 3, `buffer keeps snapshots in a night run (${cost.buffer} after 6 s)`);

// 4. Benches never offer.
await page.evaluate(() => window.__wp.startBench(8));
await page.waitForTimeout(4000);
const bench = await page.evaluate(() => ({ crashes: window.__wp.crash.crashes, phase: window.__wp.crash.phase, buf: window.__wp.sim.crashBufferTicks() }));
check(bench.phase === "idle" && bench.buf.length === 0, `bench: no buffer, no offer (${bench.crashes} crashes seen in total)`);

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
console.log(failures ? `crash replay: ${failures} check(s) failed` : "crash replay: all checks passed");
process.exit(failures ? 1 : 0);
