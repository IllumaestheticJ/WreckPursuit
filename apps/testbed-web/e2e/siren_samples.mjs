// Recorded police sirens through the real playback path (owner feedback "police sirens seem
// unrealistic — use real sounds").
//
// Usage: node e2e/siren_samples.mjs [--trace trace.json] [--source recorded|procedural]
//                                   [--pack <siren pack>] [--wav <dir>] [--label <name>]
//   trace: `cargo run -p wp_audio --example siren_trace -- trace.json` (generated when omitted).
//   --pack renders the trace's siren commands from another siren pack (before / after renders);
//   --wav writes each case's render as <dir>/<label>_<case>.wav.
//
// The director's command stream for a police cruiser (wail, yelp, phaser, hi-lo held at 30 m;
// a hi-lo pass-by at 27 m/s) is rendered through the testbed's VoicePool and siren rack into an
// OfflineAudioContext (stereo, HRTF-panned like the game; Police bus only, so the car's own engine
// loops do not mix into the seam checks). Checks, recorded mode:
//   - the recordings decode and play (no fallback to the synth),
//   - cadence: wail 4–6 s per cycle, yelp 2.5–4 sweeps / s, phaser > 8 sweeps / s, hi-lo tones
//     0.5–0.8 s each,
//   - loop seams: no click (high-frequency burst) and no dropout (level dip) at any loop wrap,
//   - pass-by: pitch up approaching, down receding (Doppler on the playback rate), loudest near
//     the closest approach.
import { spawnSync, spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const repo = join(root, "../..");
const arg = (name, def) => {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : def;
};
const source = arg("source", "recorded");
const packOverride = arg("pack", null);
const wavDir = arg("wav", null);
const label = arg("label", source);
let tracePath = arg("trace", null);
if (!tracePath) {
  tracePath = join(mkdtempSync(join(tmpdir(), "wp-siren-")), "siren.json");
  const r = spawnSync("cargo", ["run", "-q", "-p", "wp_audio", "--example", "siren_trace", "--", tracePath], {
    cwd: repo,
    stdio: "inherit",
  });
  if (r.status !== 0) throw new Error("siren_trace failed");
}
const trace = JSON.parse(readFileSync(tracePath, "utf8"));
if (packOverride) {
  for (const c of trace.cases)
    for (const f of c.frames) for (const cmd of f.commands) if (cmd.Play && cmd.Play.cue.startsWith("siren/")) cmd.Play.pack = packOverride;
}
const pack = packOverride ?? trace.siren_pack;

const PORT = 5207;
const URL = `http://localhost:${PORT}/`;
// Own process group: stopping it also stops vite under npx (a leftover vite on the port would
// serve the next run).
const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe", detached: true });
const stopServer = () => {
  try {
    process.kill(-server.pid, "SIGTERM");
  } catch {}
};
process.on("exit", stopServer);
async function waitForServer() {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(URL + "data/audio/audio_config.json")).ok) return;
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

const bundle = await build({
  stdin: {
    contents: `export * from "./voices"; export { SirenRack } from "./siren_rack";`,
    resolveDir: join(root, "src/audio"),
    loader: "ts",
  },
  bundle: true,
  write: false,
  format: "iife",
  globalName: "WPA",
  target: "es2022",
});

await waitForServer();
const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--autoplay-policy=no-user-gesture-required"],
});
const page = await browser.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
if (process.env.DEBUG) page.on("console", (m) => console.log("page:", m.text()));
page.on("console", (m) => m.type() === "error" && !m.text().startsWith("Failed to load resource") && errors.push(m.text()));
page.on("response", (r) => !r.ok() && !r.url().endsWith("/favicon.ico") && errors.push(`${r.status()} ${r.url()}`));
await page.goto(URL + "data/audio/audio_config.json");
await page.setContent(`<html><head><base href="${URL}"></head><body></body></html>`);
await page.addScriptTag({ content: bundle.outputFiles[0].text });

const SR = 44100;
const renders = {};
for (const c of trace.cases) {
  const secs = c.frames.length / 60 + 0.4;
  if (process.env.DEBUG) console.log("rendering", c.name);
  const r = await page.evaluate(
    async ({ lib, listener, frames, secs, SR, source }) => {
      const ctx = new OfflineAudioContext(2, Math.ceil(secs * SR), SR);
      const mixer = new WPA.AudioMixer(ctx);
      // Sirens only: the police car's engine and tyres have loop seams of their own.
      for (const b of ["Engine", "World", "Ui", "Music"]) mixer.buses[b].duck.disconnect();
      const pool = new WPA.VoicePool(ctx, mixer, lib);
      pool.source = source;
      const rack = new WPA.SirenRack(ctx, mixer, lib);
      pool.router = rack;
      for (let i = 0; i < 600 && pool.sampleStatus().loading > 0; i++) await new Promise((r) => setTimeout(r, 50));
      const status = pool.sampleStatus();
      WPA.setListener(ctx, ...listener);
      const q = 128 / SR;
      const plays = [];
      const errs = [];
      const engineVoices = new Set();
      frames.forEach((f, k) => {
        const t = Math.max(q, Math.round(f.t / q) * q);
        ctx.suspend(t).then(() => {
          try {
            // Same order as the game's AudioSystem.apply (index.ts).
            for (const cmd of f.commands) {
              // Siren checks only: the car's engine (procedural for police since 0.12.34) is not
              // played, so the play counters and the render hold the siren alone.
              const v = cmd.Play?.voice ?? cmd.Set?.voice ?? cmd.Stop?.voice;
              if (cmd.Play?.bus === "Engine" || (v && engineVoices.has(`${v.entity}:${v.slot}`))) {
                if (cmd.Play) engineVoices.add(`${v.entity}:${v.slot}`);
                continue;
              }
              if (cmd.Play) {
                const p = cmd.Play;
                rack.noteVoice(p.voice, p.pack);
                pool.play(p.voice, p.bus, p.pack, p.cue, p.looped, rack.adjust(p.voice, p.params), p.synth);
                if (p.cue.startsWith("siren/") && p.looped)
                  plays.push({
                    t,
                    cue: p.cue,
                    pack: p.pack,
                    synth: p.synth,
                    pitch: p.params.pitch,
                  });
              } else if (cmd.Set) pool.set(cmd.Set.voice, rack.adjust(cmd.Set.voice, cmd.Set.params));
              else if (cmd.Stop) {
                rack.forget(cmd.Stop.voice);
                pool.stop(cmd.Stop.voice, cmd.Stop.fade_s);
              } else if (cmd.Duck) mixer.duck(cmd.Duck.bus, cmd.Duck.gain, cmd.Duck.fade_s);
            }
            pool.collect();
            if (k === frames.length - 1) pool.stopAll();
          } catch (e) {
            errs.push(`frame ${k}: ${e && e.stack ? e.stack : e}`);
          } finally {
            ctx.resume();
          }
        });
      });
      // Watchdog: a stalled render fails the run instead of hanging it.
      const buf = await Promise.race([
        ctx.startRendering(),
        new Promise((_, rej) => setTimeout(() => rej(new Error(`render stalled at ${ctx.currentTime.toFixed(2)} s`)), 240000)),
      ]);
      // Loop length and this unit's variation, to locate the loop wraps in the render.
      const loops = [];
      for (const p of plays) {
        const def = lib.packs[p.pack]?.cues[p.cue];
        const file = def && "Samples" in def.source ? def.source.Samples.files[0] : null;
        let dur = null;
        if (file && source === "recorded") {
          // The loop as the pool plays it: the file's real length when the pool trims the codec
          // tail (WPA.oggVorbisLength), else whatever the browser decodes.
          const ab = await (await fetch(`data/audio/packs/${p.pack}/${file}`)).arrayBuffer();
          const len = WPA.oggVorbisLength?.(ab);
          dur = len ? len.samples / len.rate : (await new OfflineAudioContext(1, 1, SR).decodeAudioData(ab)).duration;
        }
        loops.push({
          ...p,
          dur,
          vary: WPA.sirenLoopVariation?.(lib, p.pack, p.cue, p.synth) ?? {
            rate: 1,
            offset: 0,
          },
        });
      }
      // Float32 PCM as base64 (a JSON array of numbers is ~5× larger to pass back).
      const b64 = (a) => {
        const u = new Uint8Array(a.buffer, a.byteOffset, a.byteLength);
        let str = "";
        for (let i = 0; i < u.length; i += 0x8000) str += String.fromCharCode(...u.subarray(i, i + 0x8000));
        return btoa(str);
      };
      return { status, errs, played: { ...pool.played }, loops, left: b64(buf.getChannelData(0)), right: b64(buf.getChannelData(1)) };
    },
    {
      lib: trace.library,
      listener: trace.listener,
      frames: c.frames,
      secs,
      SR,
      source,
    },
  );
  const pcm = (b) => {
    const u = Buffer.from(b, "base64");
    return new Float32Array(u.buffer, u.byteOffset, u.byteLength / 4);
  };
  r.left = pcm(r.left);
  r.right = pcm(r.right);
  r.mono = r.left.map((v, i) => 0.5 * (v + r.right[i]));
  renders[c.name] = r;
  for (const e of r.errs) errors.push(`${c.name}: ${e}`);
}
await browser.close();
stopServer();

// ---------- analysis (node) ----------
function fft(re, im) {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) {
      [re[i], re[j]] = [re[j], re[i]];
      [im[i], im[j]] = [im[j], im[i]];
    }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const a = (-2 * Math.PI) / len;
    for (let i = 0; i < n; i += len)
      for (let k = 0; k < len / 2; k++) {
        const c = Math.cos(a * k),
          s = Math.sin(a * k);
        const ur = re[i + k],
          ui = im[i + k];
        const vr = re[i + k + len / 2] * c - im[i + k + len / 2] * s;
        const vi = re[i + k + len / 2] * s + im[i + k + len / 2] * c;
        re[i + k] = ur + vr;
        im[i + k] = ui + vi;
        re[i + k + len / 2] = ur - vr;
        im[i + k + len / 2] = ui - vi;
      }
  }
}
/** Peak frequency (lo..hi Hz, parabolic) per hop. */
function peakTrack(x, win, hop, lo, hi, t0 = 0, t1 = Infinity) {
  const out = [];
  const w = new Float64Array(win).map((_, i) => 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (win - 1)));
  const s0 = Math.round(t0 * SR),
    s1 = Math.min(x.length - win, Math.round(t1 * SR));
  for (let s = s0; s <= s1; s += hop) {
    const re = new Float64Array(win),
      im = new Float64Array(win);
    for (let i = 0; i < win; i++) re[i] = x[s + i] * w[i];
    fft(re, im);
    const kLo = Math.ceil((lo * win) / SR),
      kHi = Math.floor((hi * win) / SR);
    let k = kLo,
      best = -1;
    const mag = (j) => Math.hypot(re[j], im[j]);
    for (let j = kLo; j <= kHi; j++) if (mag(j) > best) ((best = mag(j)), (k = j));
    const a = Math.log(mag(k - 1) + 1e-12),
      b = Math.log(best + 1e-12),
      c = Math.log(mag(k + 1) + 1e-12);
    const d = a - 2 * b + c;
    out.push({
      t: (s + win / 2) / SR,
      f: ((k + (d ? (0.5 * (a - c)) / d : 0)) * SR) / win,
      m: best,
    });
  }
  return out;
}
/** The strongest lag, or a sub-multiple of it (½, ⅓) that correlates nearly as well (the fundamental). */
function fundamental(rs) {
  let best = rs.reduce((a, b) => (b.r > a.r ? b : a));
  for (const div of [3, 2]) {
    const near = rs.filter((x) => Math.abs(x.lag - best.lag / div) <= 1);
    const sub = near.length ? near.reduce((a, b) => (b.r > a.r ? b : a)) : null;
    if (sub && sub.r >= 0.9 * best.r) {
      best = sub;
      break;
    }
  }
  return best;
}
/**
 * Fundamental period of a track's values within [minS, maxS] from its autocorrelation (see
 * `fundamental`).
 */
function period(track, minS, maxS) {
  const v = track.map((p) => p.f);
  const mean = v.reduce((a, b) => a + b, 0) / v.length;
  const d = v.map((x) => x - mean);
  const dt = track[1].t - track[0].t;
  const r0 = d.reduce((a, x) => a + x * x, 0);
  const rs = [];
  const lo = Math.ceil(minS / dt),
    hi = Math.min(Math.floor(maxS / dt), d.length - 2);
  for (let lag = lo; lag <= hi; lag++) {
    let s = 0;
    for (let i = 0; i + lag < d.length; i++) s += d[i] * d[i + lag];
    rs.push({ lag, r: (s / r0) * (d.length / (d.length - lag)) });
  }
  const peak = fundamental(rs);
  return { s: peak.lag * dt, r: peak.r };
}
/**
 * Repetition period from the spectra themselves (any timbre: harmonics of a horn speaker
 * confuse a peak tracker): mean cosine similarity of magnitude spectra (lo..hi Hz) `lag`
 * frames apart; the fundamental (see `fundamental`) within [minS, maxS].
 */
function specPeriod(x, win, hop, lo, hi, t0, t1, minS, maxS) {
  const frames = [];
  const w = new Float64Array(win).map((_, i) => 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (win - 1)));
  const kLo = Math.ceil((lo * win) / SR),
    kHi = Math.floor((hi * win) / SR);
  for (let s = Math.round(t0 * SR); s + win <= Math.min(x.length, Math.round(t1 * SR)); s += hop) {
    const re = new Float64Array(win),
      im = new Float64Array(win);
    for (let i = 0; i < win; i++) re[i] = x[s + i] * w[i];
    fft(re, im);
    const m = new Float64Array(kHi - kLo + 1);
    let n = 0;
    for (let k = kLo; k <= kHi; k++) ((m[k - kLo] = Math.hypot(re[k], im[k])), (n += m[k - kLo] ** 2));
    n = Math.sqrt(n) + 1e-12;
    frames.push(m.map((v) => v / n));
  }
  const dt = hop / SR;
  const rs = [];
  for (let lag = Math.ceil(minS / dt); lag <= Math.floor(maxS / dt) && lag < frames.length - 1; lag++) {
    let acc = 0;
    for (let i = 0; i + lag < frames.length; i++) {
      let d = 0;
      for (let k = 0; k < frames[i].length; k++) d += frames[i][k] * frames[i + lag][k];
      acc += d;
    }
    rs.push({ lag, r: acc / (frames.length - lag) });
  }
  const peak = fundamental(rs);
  // Contrast: similarity at the period vs the median lag (a steady tone has no cadence).
  const med = [...rs].sort((a, b) => a.r - b.r)[Math.floor(rs.length / 2)].r;
  return { s: peak.lag * dt, r: peak.r, contrast: peak.r - med };
}
/** Mean log-magnitude spectrum (8192-point) of [t0, t1). */
function spectrum(x, t0, t1) {
  const win = 8192,
    acc = new Float64Array(win / 2);
  let n = 0;
  for (let s = Math.round(t0 * SR); s + win <= Math.min(x.length, Math.round(t1 * SR)); s += win / 2, n++) {
    const re = new Float64Array(win),
      im = new Float64Array(win);
    for (let i = 0; i < win; i++) re[i] = x[s + i] * (0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (win - 1)));
    fft(re, im);
    for (let k = 0; k < win / 2; k++) acc[k] += Math.hypot(re[k], im[k]);
  }
  return acc.map((v) => Math.log(v / Math.max(n, 1) + 1e-9));
}
/** Frequency scale factor s (0.9..1.12, Doppler range; wider would alias the two hi-lo tones) mapping spectrum `a` onto `b`: b(f) ≈ a(f / s). */
function scaleFactor(a, b, lo = 300, hi = 4000) {
  const bin = SR / 8192;
  let best = { s: 1, r: -2 };
  for (let s = 0.9; s <= 1.12; s += 0.0005) {
    const xs = [],
      ys = [];
    for (let k = Math.ceil(lo / bin); k <= Math.floor(hi / bin); k++) {
      const src = k / s;
      const i = Math.floor(src),
        f = src - i;
      if (i + 1 >= a.length) break;
      xs.push(a[i] * (1 - f) + a[i + 1] * f);
      ys.push(b[k]);
    }
    const mx = xs.reduce((p, q) => p + q, 0) / xs.length,
      my = ys.reduce((p, q) => p + q, 0) / ys.length;
    let sxy = 0,
      sxx = 0,
      syy = 0;
    for (let i = 0; i < xs.length; i++) ((sxy += (xs[i] - mx) * (ys[i] - my)), (sxx += (xs[i] - mx) ** 2), (syy += (ys[i] - my) ** 2));
    const r = sxy / Math.sqrt(sxx * syy + 1e-20);
    if (r > best.r) best = { s, r };
  }
  return best;
}
const pct = (a, p) => {
  const s = [...a].sort((x, y) => x - y);
  return s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))];
};
/** Click detector: >5 kHz energy relative to the local level, 3 ms windows. */
function hfRatio(x) {
  // 4th-order high-pass (two RBJ biquads at 5 kHz).
  const biquad = (inp) => {
    const w0 = (2 * Math.PI * 5000) / SR,
      al = Math.sin(w0) / (2 * Math.SQRT1_2),
      cs = Math.cos(w0);
    const b0 = (1 + cs) / 2,
      b1 = -(1 + cs),
      b2 = (1 + cs) / 2,
      a0 = 1 + al,
      a1 = -2 * cs,
      a2 = 1 - al;
    const out = new Float64Array(inp.length);
    let x1 = 0,
      x2 = 0,
      y1 = 0,
      y2 = 0;
    for (let i = 0; i < inp.length; i++) {
      const y = (b0 * inp[i] + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2) / a0;
      x2 = x1;
      x1 = inp[i];
      y2 = y1;
      y1 = y;
      out[i] = y;
    }
    return out;
  };
  const h = biquad(biquad(x));
  const w = Math.round(0.003 * SR);
  const out = [];
  for (let s = 0; s + w <= x.length; s += Math.round(w / 2)) {
    let eh = 0,
      ex = 0;
    for (let i = s; i < s + w; i++) ((eh += h[i] * h[i]), (ex += x[i] * x[i]));
    out.push({
      t: (s + w / 2) / SR,
      r: Math.sqrt(eh / (ex + 1e-12)),
      lvl: Math.sqrt(ex / w),
    });
  }
  return out;
}
function writeWav(path, l, r) {
  const n = l.length,
    b = Buffer.alloc(44 + n * 4);
  b.write("RIFF", 0);
  b.writeUInt32LE(36 + n * 4, 4);
  b.write("WAVEfmt ", 8);
  b.writeUInt32LE(16, 16);
  b.writeUInt16LE(1, 20);
  b.writeUInt16LE(2, 22);
  b.writeUInt32LE(SR, 24);
  b.writeUInt32LE(SR * 4, 28);
  b.writeUInt16LE(4, 32);
  b.writeUInt16LE(16, 34);
  b.write("data", 36);
  b.writeUInt32LE(n * 4, 40);
  for (let i = 0; i < n; i++) {
    b.writeInt16LE(Math.round(Math.max(-1, Math.min(1, l[i])) * 32767), 44 + i * 4);
    b.writeInt16LE(Math.round(Math.max(-1, Math.min(1, r[i])) * 32767), 46 + i * 4);
  }
  writeFileSync(path, b);
}

console.log(`siren pack ${pack}, source ${source}`);
const st = Object.values(renders)[0].status;
if (source === "recorded")
  check(
    st.failed.length === 0 && st.loading === 0,
    `samples decoded (${st.loaded}/${st.total}, failed: ${st.failed.join(", ") || "none"})`,
  );
const recordedPack = Object.values(trace.library.packs[pack]?.cues ?? {}).some((c) => "Samples" in c.source);
for (const [name, r] of Object.entries(renders)) {
  if (wavDir) {
    mkdirSync(wavDir, { recursive: true });
    writeWav(join(wavDir, `${label}_${name}.wav`), r.left, r.right);
  }
  if (source === "recorded" && recordedPack)
    check(
      r.played.samples > 0 && r.played.synth === 0,
      `${name}: plays the recording (${r.played.samples} sample plays, ${r.played.synth} synth plays)`,
    );
}

const settle = 0.3;
const sec = (name) => renders[name].frames ?? trace.cases.find((c) => c.name === name).frames.length / 60;
// Wail: peak track 600-2000 Hz, 20 ms hops.
{
  const tr = peakTrack(renders.wail.mono, 4096, 882, 500, 2000, settle, sec("wail"));
  const p = period(tr, 2.5, 7);
  const fs = tr.map((x) => x.f);
  check(
    p.s >= 4 && p.s <= 6 && p.r > 0.5,
    `wail: ${p.s.toFixed(2)} s per cycle (r ${p.r.toFixed(2)}), ${pct(fs, 5).toFixed(0)}–${pct(fs, 95).toFixed(0)} Hz`,
  );
}
{
  const tr = peakTrack(renders.yelp.mono, 1024, 220, 500, 2000, settle, sec("yelp"));
  const p = period(tr, 0.15, 0.8);
  check(
    p.s > 0 && 1 / p.s >= 2.5 && 1 / p.s <= 4 && p.r > 0.5,
    `yelp: ${(1 / p.s).toFixed(2)} sweeps / s (${(p.s * 1000).toFixed(0)} ms, r ${p.r.toFixed(2)})`,
  );
}
{
  const p = specPeriod(renders.phaser.mono, 512, 110, 300, 4000, settle, sec("phaser"), 0.03, 0.2);
  check(
    1 / p.s > 8 && p.contrast > 0.05,
    `phaser: ${(1 / p.s).toFixed(1)} sweeps / s (${(p.s * 1000).toFixed(0)} ms, similarity ${p.r.toFixed(2)}, contrast ${p.contrast.toFixed(2)})`,
  );
}
{
  const p = specPeriod(renders.hilo.mono, 2048, 441, 300, 4000, settle, sec("hilo"), 0.6, 2.5);
  check(
    p.s / 2 >= 0.5 && p.s / 2 <= 0.8 && p.contrast > 0.05,
    `hi-lo: ${(p.s / 2).toFixed(3)} s per tone (cycle ${p.s.toFixed(3)} s, similarity ${p.r.toFixed(2)}, contrast ${p.contrast.toFixed(2)})`,
  );
}
// Seams: the worst high-frequency burst within 5 ms of a loop wrap vs the steady part's 99.9th
// percentile (no wrap within 10 ms) — a click is a burst far above anything else.
if (source === "recorded" && recordedPack) {
  for (const name of ["wail", "yelp", "phaser", "hilo"]) {
    const r = renders[name];
    const lp = r.loops[0];
    const hf = hfRatio(r.mono).filter((w) => w.t > lp.t + 0.1 && w.t < sec(name) - 0.05);
    const loopS = lp.dur / (lp.vary.rate * lp.pitch);
    const firstWrap = lp.t + (lp.dur * (1 - lp.vary.offset)) / (lp.vary.rate * lp.pitch);
    const wraps = [];
    for (let t = firstWrap; t < sec(name); t += loopS) wraps.push(t);
    const near = (w, d) => wraps.some((t) => Math.abs(w.t - t) < d);
    const atWrap = Math.max(...hf.filter((w) => near(w, 0.005)).map((w) => w.r));
    const steady = pct(
      hf.filter((w) => !near(w, 0.01)).map((w) => w.r),
      99.9,
    );
    // Dropout: the quietest 3 ms around a wrap vs the quietest 1 % of the steady part (a loop
    // carrying a codec tail past its real end dips toward silence at every wrap).
    const dipAtWrap = Math.min(...hf.filter((w) => near(w, 0.025)).map((w) => w.lvl));
    const steadyLow = pct(
      hf.filter((w) => !near(w, 0.03)).map((w) => w.lvl),
      1,
    );
    check(
      wraps.length >= 1 && atWrap <= Math.max(steady * 2, 0.01) && dipAtWrap >= 0.5 * steadyLow,
      `${name}: loop seam click-free (${wraps.length} wraps; HF at wrap ${atWrap.toFixed(4)} vs steady p99.9 ${steady.toFixed(4)}; level at wrap ${(20 * Math.log10(dipAtWrap / steadyLow)).toFixed(1)} dB re steady p1; loop ${loopS.toFixed(3)} s, rate ×${lp.vary.rate.toFixed(4)}, start ${(lp.vary.offset * 100).toFixed(0)} %)`,
    );
  }
}
// Pass-by: hi-lo, Doppler from the playback rate: the frequency scale between the parked hi-lo
// and the pass-by's first / last 2 s (whole tone cycles, so both tones weigh in equally).
{
  const pass = trace.pass;
  const mid = pass.secs / 2;
  const still = spectrum(renders.hilo.mono, settle, sec("hilo"));
  const up = scaleFactor(still, spectrum(renders.passby.mono, 0.3, 0.3 + 2.9));
  const down = scaleFactor(still, spectrum(renders.passby.mono, pass.secs - 3.0, pass.secs - 0.1));
  const c = 343;
  check(
    up.s > 1.03 && down.s < 0.97,
    `pass-by Doppler: ×${up.s.toFixed(3)} approaching (r ${up.r.toFixed(2)}), ×${down.s.toFixed(3)} receding (r ${down.r.toFixed(2)}); far-field model ×${(c / (c - pass.speed)).toFixed(3)} / ×${(c / (c + pass.speed)).toFixed(3)}`,
  );
  const lvl = (t0, t1) => {
    const m = renders.passby.mono;
    let s = 0,
      n = 0;
    for (let i = Math.round(t0 * SR); i < Math.round(t1 * SR); i++) ((s += m[i] * m[i]), n++);
    return 10 * Math.log10(s / n + 1e-20);
  };
  const near = lvl(mid - 0.5, mid + 0.5),
    far = lvl(0.3, 1.3);
  check(
    near > far + 6,
    `pass-by level: ${near.toFixed(1)} dB at the closest approach vs ${far.toFixed(1)} dB at ${(pass.speed * (mid - 0.8)).toFixed(0)} m`,
  );
}
check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 3).join(" | ") : ""}`);
console.log(failures ? `\n${failures} check(s) failed` : "\nall siren sample checks passed");
process.exit(failures ? 1 : 0);
