// Siren v2 end-to-end check (playtest row 82) in headless Chromium.
// Usage: node e2e/siren.mjs  (needs the wasm package built first, like e2e/run.mjs).
//
// 1. Offline renders of the real synth (OfflineAudioContext): the wail's spectral peak sweeps
//    650 → 1600 Hz and back, the yelp sweeps ~3.3 times a second, Doppler moves the hi-lo tones
//    up for an approaching car and down for a receding one, whoop and air horn sound, and no
//    pitch curve is ever rejected (the playtest row-30 class of bug; CurvePlan fuzzed too).
// 2. Live: city chase, player parked in front of a patrol car with heat → the pursuing unit's
//    siren voice plays on the Police bus (cue-driven), forced pattern changes are heard and
//    crossfade, no page / audio errors, audio frame cost stays small.
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 5196;
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

// ----- spectral helpers (node side) ------------------------------------------------------------

const SR = 48000;
/** Goertzel power at `f` over `x[i0 .. i0+n)` with a Hann window. */
function power(x, i0, n, f) {
  const w = (2 * Math.PI * f) / SR;
  const c = 2 * Math.cos(w);
  let s1 = 0;
  let s2 = 0;
  for (let i = 0; i < n; i++) {
    const h = 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (n - 1));
    const s0 = x[i0 + i] * h + c * s1 - s2;
    s2 = s1;
    s1 = s0;
  }
  return s1 * s1 + s2 * s2 - c * s1 * s2;
}
/** Strongest frequency in [lo, hi] (5 Hz steps, then 1 Hz refinement) of a window. */
function peak(x, i0, n, lo, hi) {
  let best = lo;
  let bp = -1;
  for (let f = lo; f <= hi; f += 5) {
    const p = power(x, i0, n, f);
    if (p > bp) [bp, best] = [p, f];
  }
  for (let f = best - 5; f <= best + 5; f += 1) {
    const p = power(x, i0, n, f);
    if (p > bp) [bp, best] = [p, f];
  }
  return best;
}
const rms = (x) => Math.sqrt(x.reduce((a, v) => a + v * v, 0) / Math.max(1, x.length));

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist", "--autoplay-policy=no-user-gesture-required"],
});
const page = await (await browser.newContext({ viewport: { width: 1400, height: 860 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => (m.type() === "error" || /siren curve rejected/.test(m.text())) && errors.push(m.text()));
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true && window.__wpAudio, null, { timeout: 60000 });

const render = (opts) => page.evaluate(async (o) => Array.from(await window.__wpAudio.renderSiren(o)), opts);
const WAIL = { lo_hz: 650, hi_hz: 1600, cycle_s: 4, rise_frac: 0.6, two_tone: 0 };
const YELP = { lo_hz: 650, hi_hz: 1600, cycle_s: 0.3, rise_frac: 0.5, two_tone: 0 };
const HILO = { lo_hz: 770, hi_hz: 960, cycle_s: 1.1, rise_frac: 0.5, two_tone: 1 };

// 1a. Wail: the peak track rises to ~1600 Hz around 2.4 s and falls back toward 650 Hz.
{
  const x = await render({ cadence: WAIL, seconds: 4.2 });
  const track = [];
  for (let t = 0.15; t < 4.1; t += 0.1) track.push([t, peak(x, Math.floor(t * SR), 2048, 560, 1800)]);
  const fs = track.map((p) => p[1]);
  const [tMax, fMax] = track.reduce((a, b) => (b[1] > a[1] ? b : a));
  const inRange = fs.every((f) => f >= 650 * 0.97 && f <= 1600 * 1.03);
  let rising = 0;
  for (let i = 1; i < track.length && track[i][0] < 2.2; i++) if (fs[i] >= fs[i - 1] - 10) rising++;
  check(inRange, `wail peak stays in 650–1600 Hz (${Math.min(...fs)}–${Math.max(...fs)} Hz)`);
  check(fMax > 1500 && Math.abs(tMax - 2.4) < 0.35, `wail tops out at ${fMax} Hz at ${tMax.toFixed(2)} s (expected ~1600 Hz at 2.4 s)`);
  check(rising >= track.filter((p) => p[0] < 2.2).length - 2, `wail rises monotonically for the first 2.2 s (${rising} steps)`);
  check(Math.min(...fs.slice(-3)) < 900, `wail falls back toward 650 Hz (${Math.min(...fs.slice(-3))} Hz at the end of the cycle)`);
  // Every point matches the cadence model (window centre, ±4 % + 15 Hz for the window smear).
  const model = (t) => {
    const p = (t / WAIL.cycle_s) % 1;
    const r = WAIL.rise_frac;
    const smooth = (u) => u * u * (3 - 2 * u);
    const x = p < r ? smooth(p / r) : 1 - smooth((p - r) / (1 - r));
    return WAIL.lo_hz + (WAIL.hi_hz - WAIL.lo_hz) * x;
  };
  const off = track.filter(([t, f]) => Math.abs(f - model(t + 1024 / SR)) > model(t) * 0.04 + 15);
  check(off.length === 0, `wail track follows the cadence model (${off.length} of ${track.length} points off${off.length ? ": " + off.slice(0, 3).map(([t, f]) => `${t.toFixed(2)} s ${f} Hz vs ${model(t).toFixed(0)}`).join(", ") : ""})`);
}
// 1b. Yelp: ~3.3 sweeps per second (count upward crossings of the mid frequency).
{
  const x = await render({ cadence: YELP, seconds: 2.2 });
  const fs = [];
  for (let t = 0.15; t < 2.15; t += 0.01) fs.push(peak(x, Math.floor(t * SR), 1024, 560, 1800));
  let ups = 0;
  for (let i = 1; i < fs.length; i++) if (fs[i - 1] < 1125 && fs[i] >= 1125) ups++;
  check(ups >= 5 && ups <= 8, `yelp sweeps ${ups} times in 2 s (expected ~6.7)`);
  check(Math.max(...fs) > 1400 && Math.min(...fs) < 850, `yelp spans ${Math.min(...fs)}–${Math.max(...fs)} Hz`);
}
// 1c. Doppler: the hi-lo high tone (960 Hz) moves up approaching, down receding.
{
  const tone = async (doppler) => {
    const x = await render({ cadence: HILO, doppler, seconds: 0.6 });
    return peak(x, Math.floor(0.2 * SR), 8192, 600, 1300);
  };
  const [still, toward, away] = [await tone(1), await tone(343 / 313), await tone(343 / 373)];
  check(Math.abs(still - 960) <= 6, `hi-lo high tone ${still} Hz (960)`);
  check(Math.abs(toward - 960 * (343 / 313)) <= 10 && toward > still, `approaching at 30 m/s: ${toward} Hz (expected ${(960 * 343 / 313).toFixed(0)}, up)`);
  check(Math.abs(away - 960 * (343 / 373)) <= 10 && away < still, `receding at 30 m/s: ${away} Hz (expected ${(960 * 343 / 373).toFixed(0)}, down)`);
}
// 1d. Whoop and air horn sound; whoop sweeps up then down.
{
  const w = await render({ kind: 1, seconds: 0.7 });
  const h = await render({ kind: 2, seconds: 0.6 });
  const wTrack = [0.05, 0.2, 0.45].map((t) => peak(w, Math.floor(t * SR), 1024, 500, 1800));
  check(rms(w) > 0.02 && wTrack[1] > wTrack[0] && wTrack[2] < wTrack[1], `whoop chirp: rms ${rms(w).toFixed(3)}, ${wTrack.join(" → ")} Hz`);
  check(rms(h) > 0.02 && rms(h.slice(Math.floor(0.55 * SR))) < rms(h) * 0.2, `air-horn blast: rms ${rms(h).toFixed(3)}, ends by 0.55 s`);
  check([...w, ...h].every(Number.isFinite), "renders are finite (no NaN)");
}
// 1e. CurvePlan fuzz in the browser (doubles, like the live scheduler).
{
  const r = await page.evaluate(() => {
    const Plan = window.__wpAudio.SirenCurvePlan;
    let bad = 0;
    let n = 0;
    for (const [origin, seg] of [[0, 0.1], [12345.678901, 0.1], [0.3, 0.033333], [1e5 + 0.1, 0.05]]) {
      const plan = new Plan(origin, seg, 0.001);
      let now = origin;
      let prevEnd = -Infinity;
      for (let f = 0; f < 200000; f++) {
        now += f % 997 === 0 ? 0.1 + Math.random() * 0.4 : 1 / 60 + (Math.random() - 0.5) * 0.004;
        for (const c of plan.plan(now, 0.25)) {
          n++;
          if (c.start < prevEnd + plan.gap * 0.5 || c.start < now - 1e-9) bad++;
          prevEnd = c.start + c.dur;
        }
      }
    }
    return { bad, n };
  });
  check(r.bad === 0 && r.n > 100000, `curve plan never overlaps (${r.n} segments, ${r.bad} bad)`);
}

// 2. Live chase.
await page.keyboard.press("KeyX");
await page.waitForFunction(() => window.__wpAudio?.running(), null, { timeout: 10000 });
check(true, "AudioContext running after a key press");
// Baseline audio frame cost while free-driving (no sirens).
await page.keyboard.down("KeyW");
await page.waitForTimeout(3000);
await page.keyboard.up("KeyW");
const baseMs = await page.evaluate(() => window.__wpAudio.frameMs);

await page.evaluate(() => window.__wp.startCityChase("debug_sedan", "Arcade"));
const parked = await page
  .waitForFunction(
    () => {
      const wp = window.__wp;
      try {
        wp.sim.debugParkNearPolice(wp.app.player, 40);
        wp.sim.debugSetHeat(wp.app.player, 30);
        return true;
      } catch {
        return false;
      }
    },
    null,
    { timeout: 120000, polling: 100 },
  )
  .then(() => true)
  .catch(() => false);
check(parked, "player parked in front of a patrol car with heat (pursuit)");
const live = await page
  .waitForFunction(() => (window.__wpAudio.sirens?.stats().playing ?? 0) > 0, null, { timeout: 120000, polling: 200 })
  .then(() => true)
  .catch(() => false);
const s1 = await page.evaluate(() => ({
  stats: window.__wpAudio.sirens?.stats(),
  space: window.__wpAudio.sirens?.last,
  counts: { ...window.__wpAudio.cueCounts },
  police: window.__wpAudio.stats()?.per_bus?.Police ?? 0,
}));
const sirenPlays = Object.entries(s1.counts).filter(([k]) => k.startsWith("siren/")).reduce((a, [, v]) => a + v, 0);
check(live && sirenPlays > 0 && s1.police > 0, `chase start → siren voice active (${s1.stats?.playing} playing, ${sirenPlays} siren plays, police bus ${s1.police} voices, cue-driven ${JSON.stringify(s1.stats?.patterns)})`);
// Flee for a few seconds so the unit chases (cue changes as the gap opens / closes).
await page.keyboard.down("KeyW");
await page.waitForTimeout(4000);
await page.keyboard.up("KeyW");

// Forced pattern changes on a live unit: each is heard (new loop plays) and the old one fades.
const unit = await page.evaluate(() => window.__wpAudio.sirens?.last?.voices?.[0]?.entity ?? null);
const heard = [];
// Forcing the pattern a unit already plays is no change (nothing new to hear): start from the
// others (police cues often leave the unit on Yelp already).
const current = await page.evaluate(
  (e) => window.__wpAudio.sirens?.last?.voices?.find((x) => x.entity === e)?.pattern ?? null,
  unit,
);
const modes = ["Yelp", "Phaser", "HiLo", "Wail"];
modes.sort((a, b) => (a === current) - (b === current));
for (const mode of modes) {
  if (unit === null) break;
  const before = await page.evaluate((m) => window.__wpAudio.cueCounts[`siren/${m.toLowerCase()}`] ?? 0, mode);
  await page.evaluate(([e, m]) => window.__wpAudio.setSirenFor(e, m), [unit, mode]);
  const ok = await page
    .waitForFunction(
      ([e, m, b]) => {
        const a = window.__wpAudio;
        const v = a.sirens?.last?.voices?.find((x) => x.entity === e);
        return v?.pattern === m && (a.cueCounts[`siren/${m.toLowerCase()}`] ?? 0) > b;
      },
      [unit, mode, before],
      { timeout: 15000, polling: 100 },
    )
    .then(() => true)
    .catch(() => false);
  if (ok) heard.push(mode);
  await page.waitForTimeout(400);
}
if (unit !== null) await page.evaluate((e) => window.__wpAudio.setSirenFor(e, "Default"), unit);
check(heard.length === 4, `mode changes heard on unit ${unit}: ${heard.join(", ") || "none"}`);
const loops = await page.evaluate(() => window.__wpAudio.loopCounts());
check(loops && Math.abs(loops.backend - loops.director) <= 1, `no stale voices after crossfades (backend ${loops?.backend}, director ${loops?.director})`);

// Siren lab: plays, whoop + horn, pass-by, stop.
await page.evaluate(() => {
  const lab = window.__wpAudio.sirenLab;
  lab.play();
  lab.passBy();
});
await page.waitForTimeout(1500);
await page.evaluate(() => window.__wpAudio.sirenLab.stop());

// No audio errors, cost.
const end = await page.evaluate(() => ({
  st: { ...window.__wpAudio.sirenStats },
  frameMs: window.__wpAudio.frameMs,
  rackMs: window.__wpAudio.sirens?.frameMs ?? 0,
  running: window.__wpAudio.running(),
}));
check(end.st.curveErrors === 0 && end.st.curves > 0, `no rejected pitch curves (${end.st.curves} scheduled, ${end.st.curveErrors} rejected)`);
check(end.running && errors.length === 0, `no page or audio errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
check(end.rackMs < 0.5, `siren rack costs ${end.rackMs.toFixed(3)} ms / frame`);
check(end.frameMs < baseMs * 2 + 2, `audio frame ${end.frameMs.toFixed(2)} ms with sirens vs ${baseMs.toFixed(2)} ms free-driving`);

await browser.close();
server.kill("SIGTERM");
console.log(failures ? `\n${failures} check(s) failed` : "\nall siren checks passed");
process.exit(failures ? 1 : 0);
