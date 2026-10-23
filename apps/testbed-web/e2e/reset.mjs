// Reset in place (playtest row 65) check, headless Chromium (Playwright). Run by `npm run e2e`
// (run.mjs) at its end, or alone: `npm run e2e:reset` (needs `npm run wasm` first).
// Env: CHROMIUM=/path/to/chrome.
//
// Checks: after the barrier crash scenario, R puts the car upright at rest within 15 m of the
// crash point, repaired (health 100 %), and the R is a replay op that plays back without drift;
// in the night run R lands on a lane, keeps the police heat and makes the car a blinking ghost
// for 1 s; in a career run R does not
// repair (repairs are bought in the garage).
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5193;
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
const page = await context.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
const ready = () => page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 30000 });

// Player record from vehicle_states(): pos 1..3, rot 4..7 (x, y, z, w), speed 14, health 17.
const player = () =>
  page.evaluate(() => {
    const wp = window.__wp;
    const s = wp.sim.states();
    const stride = 50; // STATE_STRIDE (src/wasm/layout.ts)
    let o = 0;
    for (; o < s.length; o += stride) if (s[o] === wp.app.player) break;
    const q = [s[o + 4], s[o + 5], s[o + 6], s[o + 7]];
    return {
      pos: [s[o + 1], s[o + 2], s[o + 3]],
      upZ: 1 - 2 * (q[0] * q[0] + q[1] * q[1]),
      speed: s[o + 14],
      health: s[o + 17],
      tick: wp.sim.worldInfo()[0],
    };
  });
const dist2d = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1]);

await page.goto(URL);
await ready();
await page.evaluate(() => localStorage.removeItem("wp.career.v1"));

// 1. Barrier crash (scripted 7 s), record, R, playback.
await page.evaluate(() => window.__wp.runScenario("crash/barrier_frontal.ron"));
await page.waitForFunction(() => window.__wp.sim.worldInfo()[0] > 7 * 240 + 30, null, { timeout: 60000 });
await page.evaluate(() => window.__wp.sim.recordStart());
const crashed = await player();
await page.keyboard.press("KeyR");
await page.waitForTimeout(1200);
const after = await player();
const moved = dist2d(after.pos, crashed.pos);
check(crashed.health < 0.99, `the barrier crash damaged the car (health ${(crashed.health * 100).toFixed(0)} %)`);
check(
  after.upZ > 0.95 && moved <= 15 && Math.abs(after.speed) < 1.0 && after.health > 0.999,
  `R: upright (up·z ${after.upZ.toFixed(3)}), ${moved.toFixed(1)} m from the crash, ${after.speed.toFixed(2)} m/s, health ${(after.health * 100).toFixed(0)} %`,
);
const feed = await page.evaluate(() => document.querySelector(".feed")?.textContent ?? "");
check(feed.includes("reset in place") && feed.includes("repaired"), "the event feed notes the reset and repair");
const pb = await page.evaluate(async () => {
  const wp = window.__wp;
  const bytes = wp.stopRecording();
  wp.loadReplay(bytes);
  wp.sim.replayPlay(4);
  for (let i = 0; i < 600; i++) {
    await new Promise((r) => requestAnimationFrame(r));
    const st = wp.sim.replayStatus();
    if (st.tick >= st.end_tick) return st;
  }
  return wp.sim.replayStatus();
});
check(pb.tick >= pb.end_tick && pb.drift_count === 0, `the reset is a replay op: playback to tick ${pb.tick}/${pb.end_tick} with ${pb.drift_count} drift`);
await page.evaluate(() => window.__wp.sim.replayExit?.());

// 2. Night run: R lands on a lane near the car and keeps the heat.
await page.evaluate(() => window.__wp.startNightRun());
await page.waitForTimeout(1500);
await page.evaluate(() => window.__wp.sim.debugSetHeat(window.__wp.app.player, 0.6));
const c0 = await player();
// Heat before / after in one evaluate: separate calls let frames (and the police) run in
// between, and the heat moves on its own early in a chase.
const { heat0, r, heat1 } = await page.evaluate(() => {
  const wp = window.__wp;
  const heat0 = wp.sim.policeStatus(wp.app.player)?.heat ?? null;
  const r = wp.sim.resetInPlace(wp.app.player);
  const heat1 = wp.sim.policeStatus(wp.app.player)?.heat ?? null;
  // Sample the player's drawn visibility every frame from the reset on, while the ghost window
  // lasts: on a slow machine the whole window is a few frames, gone before a later evaluate.
  const p = wp.app.player;
  const seen = (window.__ghostSeen = new Set());
  const sample = () => {
    if (wp.sim.ghostTicks(p) === 0) return;
    seen.add(wp.app.views.get(p).root.visible);
    requestAnimationFrame(sample);
  };
  requestAnimationFrame(sample);
  return { heat0, r, heat1 };
});
await page.waitForTimeout(300);
const c1 = await player();
check(
  r.on_lane && dist2d([r.x, r.y], c0.pos) <= 15 && c1.upZ > 0.95,
  `night run: reset lands on a lane ${dist2d([r.x, r.y], c0.pos).toFixed(1)} m away, upright`,
);
check(heat0 !== null && heat0 > 0 && heat1 === heat0, `night run: heat unchanged by the reset (${heat0} → ${heat1})`);
// Ghost window: 1 s through traffic (wp_sim GhostHooks; pass-through is proven natively in
// wp_sim's tests), the car blinks meanwhile, then it is solid again.
const ghost = await page.evaluate(async () => {
  const wp = window.__wp;
  const p = wp.app.player;
  const at0 = wp.sim.ghostTicks(p);
  // Frames sampled since the reset (above) plus a few more while the window lasts.
  const seen = window.__ghostSeen;
  for (let i = 0; i < 20 && wp.sim.ghostTicks(p) > 0; i++) {
    await new Promise((r) => requestAnimationFrame(r));
    seen.add(wp.app.views.get(p).root.visible);
  }
  const t0 = performance.now();
  while (wp.sim.ghostTicks(p) > 0 && performance.now() - t0 < 8000) await new Promise((r) => setTimeout(r, 100));
  await new Promise((r) => requestAnimationFrame(r));
  // Blinked = hidden in at least one ghost frame and drawn again afterwards.
  const visible = wp.app.views.get(p).root.visible;
  return { at0, blinked: seen.has(false) && visible, after: wp.sim.ghostTicks(p), visible };
});
check(
  r.ghost_ticks === 240 && ghost.blinked && ghost.after === 0 && ghost.visible,
  `night run: the reset car is a ghost for 1 s and blinks, then solid and drawn again (ghost ${r.ghost_ticks} ticks, blinked ${ghost.blinked}, after ${ghost.after})`,
);

// 3. Career run: reset in place, no repair.
const career = await page.evaluate(() => {
  const wp = window.__wp;
  wp.careerApi.newCareer("Arcade");
  wp.careerApi.floorCar();
  wp.careerApi.startRun("free");
  return { active: wp.sim.careerRunActive(), r: wp.sim.careerRunActive() ? wp.sim.resetInPlace(wp.app.player) : null };
});
check(career.active && career.r && career.r.repaired === false, `career run: R resets in place without repairing (${JSON.stringify(career.r)})`);
await page.evaluate(() => localStorage.removeItem("wp.career.v1"));

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
console.log(failures ? `${failures} reset check(s) failed` : "all reset checks passed");
process.exit(failures ? 1 : 0);
