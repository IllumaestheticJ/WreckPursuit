// Offline render of an engine-audio trace through the testbed's real voice pool and synths.
//
// Usage:
//   cargo run --release -p wp_audio --example engine_trace -- <car> trace.csv trace.json
//   node e2e/engine_render.mjs trace.json [out.wav] [report.json] [--all-buses]
//
// Bundles src/audio/voices.ts with esbuild, opens a blank headless Chromium page and plays the
// trace's AudioCommand stream into an OfflineAudioContext, suspending at every 60 Hz frame so
// commands land at the same audio time they would in the game. Prints a per-50 ms table
// (rendered level vs the director's layer energy) and a click count; optionally writes the
// WAV and a JSON report (`windows`, `clicks`, `peak`).
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";
import { chromium } from "playwright-core";
import { chromiumPath } from "./chromium.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
// `--all-buses` renders every voice; by default only the Engine bus (the engine note alone).
const allBuses = process.argv.includes("--all-buses");
const [tracePath, wavPath, reportPath] = process.argv.slice(2).filter((a) => !a.startsWith("--"));
if (!tracePath) {
  console.error("usage: node e2e/engine_render.mjs trace.json [out.wav] [report.json]");
  process.exit(2);
}
const trace = JSON.parse(readFileSync(tracePath, "utf8"));

const bundle = await build({
  entryPoints: [join(root, "src/audio/voices.ts")],
  bundle: true,
  write: false,
  format: "iife",
  globalName: "WPA",
  target: "es2022",
});
const code = bundle.outputFiles[0].text;

const browser = await chromium.launch({
  executablePath: chromiumPath(),
  args: ["--autoplay-policy=no-user-gesture-required"],
});
const page = await browser.newPage();
page.on("pageerror", (e) => console.error("page error:", e.message));
await page.setContent("<html><body></body></html>");
await page.addScriptTag({ content: code });

const SR = 48000;
const result = await page.evaluate(
  async ({ trace, SR, allBuses }) => {
    const frames = trace.frames;
    const dur = frames[frames.length - 1].t - frames[0].t + 0.5;
    const ctx = new OfflineAudioContext(2, Math.ceil(dur * SR), SR);
    const mixer = new WPA.AudioMixer(ctx);
    const pool = new WPA.VoicePool(ctx, mixer, trace.library);
    const t0 = frames[0].t;
    const apply = (f) => {
      if (f.listener) WPA.setListener(ctx, ...f.listener);
      for (const c of f.commands) {
        if ("Play" in c) {
          const p = c.Play;
          if (!allBuses && p.bus !== "Engine") continue;
          pool.play(p.voice, p.bus, p.pack, p.cue, p.looped, p.params, p.synth);
        } else if ("Set" in c) pool.set(c.Set.voice, c.Set.params);
        else if ("Stop" in c) pool.stop(c.Stop.voice, c.Stop.fade_s);
        else if ("Duck" in c) mixer.duck(c.Duck.bus, c.Duck.gain, c.Duck.fade_s);
      }
      pool.collect();
    };
    // Frame i's commands at audio time t_i - t0 (rounded to a render quantum by the browser).
    const q = 128 / SR;
    let lastT = -1;
    for (const f of frames) {
      let t = Math.round((f.t - t0) / q) * q;
      if (t <= lastT) t = lastT + q;
      lastT = t;
      if (t === 0) {
        apply(f);
        continue;
      }
      ctx.suspend(t).then(() => {
        apply(f);
        ctx.resume();
      });
    }
    const buf = await ctx.startRendering();
    const L = buf.getChannelData(0);
    const R = buf.getChannelData(1);
    // Per-50 ms windows: RMS (dB) and a click score (largest jump of the high-passed signal
    // relative to that window's high-passed RMS; a click is a single-sample step).
    const win = Math.round(0.05 * SR);
    const windows = [];
    let peak = 0;
    let clicks = 0;
    const clickTimes = [];
    let prevM = 0;
    let prevD = 0;
    for (let s = 0; s + win <= L.length; s += win) {
      let sum = 0;
      let dsum = 0;
      let dmax = 0;
      for (let i = s; i < s + win; i++) {
        const m = 0.5 * (L[i] + R[i]);
        const d = m - prevM;
        const dd = Math.abs(d - prevD);
        prevM = m;
        prevD = d;
        sum += m * m;
        dsum += dd * dd;
        if (dd > dmax) dmax = dd;
        if (Math.abs(m) > peak) peak = Math.abs(m);
      }
      const rms = Math.sqrt(sum / win);
      const drms = Math.sqrt(dsum / win);
      const score = drms > 1e-6 ? dmax / drms : 0;
      windows.push({ t: t0 + s / SR, rms, score });
      if (score > 12 && dmax > 0.02) {
        clicks++;
        clickTimes.push(+(t0 + s / SR).toFixed(2));
      }
    }
    // 16-bit stereo PCM for listening.
    const pcm = new Int16Array(L.length * 2);
    for (let i = 0; i < L.length; i++) {
      pcm[2 * i] = Math.max(-1, Math.min(1, L[i])) * 32767;
      pcm[2 * i + 1] = Math.max(-1, Math.min(1, R[i])) * 32767;
    }
    let bin = "";
    const bytes = new Uint8Array(pcm.buffer);
    for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return { windows, clicks, clickTimes, peak, pcm: btoa(bin) };
  },
  { trace, SR, allBuses },
);
await browser.close();

if (wavPath) {
  const data = Buffer.from(result.pcm, "base64");
  const h = Buffer.alloc(44);
  h.write("RIFF", 0);
  h.writeUInt32LE(36 + data.length, 4);
  h.write("WAVEfmt ", 8);
  h.writeUInt32LE(16, 16);
  h.writeUInt16LE(1, 20);
  h.writeUInt16LE(2, 22);
  h.writeUInt32LE(SR, 24);
  h.writeUInt32LE(SR * 4, 28);
  h.writeUInt16LE(4, 32);
  h.writeUInt16LE(16, 34);
  h.write("data", 36);
  h.writeUInt32LE(data.length, 40);
  writeFileSync(wavPath, Buffer.concat([h, data]));
}
const report = { windows: result.windows, clicks: result.clicks, clickTimes: result.clickTimes, peak: result.peak };
if (reportPath) writeFileSync(reportPath, JSON.stringify(report));
console.log(`peak ${result.peak.toFixed(3)}  clicks ${result.clicks} ${result.clickTimes.slice(0, 20).join(" ")}`);
