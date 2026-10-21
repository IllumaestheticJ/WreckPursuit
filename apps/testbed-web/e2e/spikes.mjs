// Spike strips heard (playtest row 132) in headless Chromium:
//   - the flat-tyre flap synth slaps once per wheel revolution: its rate follows the wheel
//     (3 and 9 rev/s rendered offline, slap rate measured from the envelope),
//   - the recorded blow-outs (world_rec tire/burst) load and decode, and are loud (bang first,
//     quieter air rush after),
//   - no page errors.
// Usage: node e2e/spikes.mjs  (needs the wasm package built first, like e2e/run.mjs).
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 5194;
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
const page = await (await browser.newContext()).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true && window.__wpAudio, null, { timeout: 60000 });

// 1. Flap rate follows the wheel: dominant envelope periodicity (autocorrelation, 5 ms hop).
const SR = 48000;
function slapRate(x) {
  const hop = SR * 0.005;
  const env = [];
  for (let i = 0; i + hop <= x.length; i += hop) {
    let s = 0;
    for (let j = i; j < i + hop; j++) s += x[j] * x[j];
    env.push(Math.sqrt(s / hop));
  }
  const m = env.reduce((a, b) => a + b, 0) / env.length;
  const e = env.map((v) => v - m);
  let best = 0;
  let bestLag = 0;
  // Lags 40 ms .. 1 s (25 .. 1 slaps per second).
  for (let lag = 8; lag <= 200; lag++) {
    let c = 0;
    for (let i = 0; i + lag < e.length; i++) c += e[i] * e[i + lag];
    if (c > best) [best, bestLag] = [c, lag];
  }
  return 1 / (bestLag * 0.005);
}
for (const rev of [3, 9]) {
  const x = await page.evaluate(async (r) => Array.from(await window.__wpAudio.renderFlap(r, 3.0)), rev);
  const rate = slapRate(x.slice(SR * 0.3));
  const peak = x.reduce((m, v) => Math.max(m, Math.abs(v)), 0);
  check(Math.abs(rate - rev) / rev < 0.1 && peak > 0.05 && x.every(Number.isFinite), `flap at ${rev} rev/s slaps ${rate.toFixed(2)} times a second (peak ${peak.toFixed(2)})`);
}

// 2. The recorded blow-outs.
const pops = await page.evaluate(async () => {
  const ctx = new OfflineAudioContext(1, 48000, 48000);
  const out = [];
  for (const f of ["tire_burst_1.ogg", "tire_burst_2.ogg"]) {
    const r = await fetch(`data/audio/packs/world_rec/${f}`);
    const buf = await ctx.decodeAudioData(await r.arrayBuffer());
    const d = buf.getChannelData(0);
    const rms = (a, b) => {
      let s = 0;
      const i0 = Math.floor(a * buf.sampleRate);
      const i1 = Math.floor(b * buf.sampleRate);
      for (let i = i0; i < i1; i++) s += d[i] * d[i];
      return 20 * Math.log10(Math.sqrt(s / (i1 - i0)) + 1e-12);
    };
    out.push({ f, ok: r.ok, dur: buf.duration, peak: 20 * Math.log10(d.reduce((m, v) => Math.max(m, Math.abs(v)), 0)), bang: rms(0, 0.08), tail: rms(0.2, 0.6) });
  }
  return out;
});
for (const p of pops)
  check(
    p.ok && p.peak > -3 && p.bang > p.tail + 3 && p.dur > 0.3 && p.dur < 1.5,
    `${p.f}: ${p.dur.toFixed(2)} s, peak ${p.peak.toFixed(1)} dBFS, bang ${p.bang.toFixed(1)} dB over the air rush ${p.tail.toFixed(1)} dB`,
  );

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
await browser.close();
server.kill("SIGTERM");
console.log(failures ? `\n${failures} check(s) failed` : "\nall spike checks passed");
process.exit(failures ? 1 : 0);
