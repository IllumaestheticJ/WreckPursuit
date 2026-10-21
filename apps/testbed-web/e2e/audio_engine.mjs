// Engine synthesizer validation (owner row 73, "simulate accurate engine sounds").
//
// Usage: node e2e/audio_engine.mjs [sweeps.json] [--no-media] [--no-game] [--verbose]
//   sweeps.json: `cargo run -p wp_audio --example engine_sweeps -- sweeps.json` (generated
//   into a temp file when omitted). Needs no wasm build for parts 1–3 (it bundles the audio
//   backend with esbuild); part 4 drives the real testbed page and needs the wasm package.
//
// 1. Offline render, headless Chromium: every sweep (I4, I6, cross-plane V8, flat-plane V8,
//    turbo I4 × WOT idle → redline + limiter, lift-off, shift, rpm steps; damaged V8) goes
//    through the real VoicePool + engine worklet. WAVs and spectrogram JPEGs land in
//    docs/media/audio/ with a README report (skip with --no-media).
// 2. Checks on the rendered audio:
//    - pitch: the firing harmonic (rpm / 60 × cylinders / 2) tracks the crank within 1 % on
//      every rpm plateau and along the WOT sweep (median), and stands ≥ 10 dB out of its band;
//    - harmonic structure: I4 / I6 / flat-plane V8 have weak half orders (non-integer crank
//      orders ≤ −15 dB vs the firing order); the cross-plane V8 burbles (≥ −12 dB); the firing
//      order leads the I4 and I6 spectra below it;
//    - lift-off crackle on sporty cars, blow-off on the turbo lift, limiter cuts, misfires on the
//      damaged engine (worklet counters);
//    - no clicks (2nd-difference detector, as e2e/engine_render.mjs), no subnormal samples, no
//      NaN; level within 6 dB of the procedural layers on the same sweep;
//    - interior perspective is darker (less energy above 1 kHz) than exterior.
// 3. Budget: the player + 6 nearby engines in one worklet, offline: ≤ 1.5 ms of render time
//    per 128-frame block (engines minus an empty render).
// 4. In the game page: the synthesizer loads, the player's car is modelled, switching to layers
//    and back works, no page errors.
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
const verbose = process.argv.includes("--verbose");
const media = !process.argv.includes("--no-media");
// --no-game: skip part 4 (no wasm build needed).
const runGame = !process.argv.includes("--no-game");
let tracePath = process.argv.slice(2).find((a) => !a.startsWith("--"));
if (!tracePath) {
  tracePath = join(mkdtempSync(join(tmpdir(), "wp-engine-")), "sweeps.json");
  const r = spawnSync("cargo", ["run", "-q", "-p", "wp_audio", "--example", "engine_sweeps", "--", tracePath], {
    cwd: repo,
    stdio: "inherit",
  });
  if (r.status !== 0) throw new Error("engine_sweeps failed");
}
const trace = JSON.parse(readFileSync(tracePath, "utf8"));
const SR = 48000;

let failures = 0;
const check = (ok, msg) => {
  console.log(`${ok ? "PASS" : "FAIL"}  ${msg}`);
  if (!ok) failures++;
};

// ----- analysis helpers (Node) ------------------------------------------------------------------

/** In-place radix-2 FFT. */
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
    const wr = Math.cos(a);
    const wi = Math.sin(a);
    for (let i = 0; i < n; i += len) {
      let cr = 1;
      let ci = 0;
      for (let k = 0; k < len / 2; k++) {
        const ur = re[i + k];
        const ui = im[i + k];
        const vr = re[i + k + len / 2] * cr - im[i + k + len / 2] * ci;
        const vi = re[i + k + len / 2] * ci + im[i + k + len / 2] * cr;
        re[i + k] = ur + vr;
        im[i + k] = ui + vi;
        re[i + k + len / 2] = ur - vr;
        im[i + k + len / 2] = ui - vi;
        const t = cr * wr - ci * wi;
        ci = cr * wi + ci * wr;
        cr = t;
      }
    }
  }
}

/** Magnitude spectrum (dB) of x[s0, s1) with a Hann window, zero-padded to ≥ `minN`. */
function spectrum(x, s0, s1, minN = 1 << 16) {
  const len = s1 - s0;
  let n = 1;
  while (n < Math.max(len, minN)) n <<= 1;
  const re = new Float64Array(n);
  const im = new Float64Array(n);
  for (let i = 0; i < len; i++) re[i] = x[s0 + i] * (0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (len - 1)));
  fft(re, im);
  const db = new Float64Array(n / 2);
  for (let i = 0; i < n / 2; i++) db[i] = 10 * Math.log10(re[i] * re[i] + im[i] * im[i] + 1e-20);
  return { db, hz: SR / n };
}

/** Peak frequency (parabolic interpolation) and level in [lo, hi] Hz. */
function peakIn(sp, lo, hi) {
  const a = Math.max(1, Math.floor(lo / sp.hz));
  const b = Math.min(sp.db.length - 2, Math.ceil(hi / sp.hz));
  let k = a;
  for (let i = a; i <= b; i++) if (sp.db[i] > sp.db[k]) k = i;
  const y0 = sp.db[k - 1];
  const y1 = sp.db[k];
  const y2 = sp.db[k + 1];
  const d = (0.5 * (y0 - y2)) / (y0 - 2 * y1 + y2 || 1e-9);
  return { f: (k + clamp(d, -0.5, 0.5)) * sp.hz, db: y1 };
}

/** Median level (dB) of the bins in [lo, hi] Hz. */
function medianIn(sp, lo, hi) {
  const v = [];
  for (let i = Math.floor(lo / sp.hz); i <= Math.ceil(hi / sp.hz); i++) v.push(sp.db[i]);
  v.sort((p, q) => p - q);
  return v[v.length >> 1];
}

const clamp = (x, lo, hi) => Math.max(lo, Math.min(hi, x));
const median = (a) => [...a].sort((p, q) => p - q)[a.length >> 1];
const quantile = (a, q) => [...a].sort((p, r) => p - r)[Math.min(a.length - 1, Math.floor(q * a.length))];
const rmsOf = (x, s0 = 0, s1 = x.length) => {
  let s = 0;
  for (let i = s0; i < s1; i++) s += x[i] * x[i];
  return Math.sqrt(s / Math.max(1, s1 - s0));
};
const dB = (v) => 20 * Math.log10(Math.max(v, 1e-12));
/** Two cascaded one-pole low-passes at `fc` Hz. */
const lowpass = (x, fc) => {
  const k = 1 - Math.exp((-2 * Math.PI * fc) / SR);
  const y = new Float32Array(x.length);
  let a = 0;
  let b = 0;
  for (let i = 0; i < x.length; i++) {
    a += (x[i] - a) * k;
    b += (a - b) * k;
    y[i] = b;
  }
  return y;
};
/** "Body" level of [t0, t1) s: median of 10 ms RMS windows (a pop or two does not move it). */
const bodyOf = (x, t0, t1) => {
  const w = Math.round(0.01 * SR);
  const v = [];
  for (let s = Math.round(t0 * SR); s + w <= Math.round(t1 * SR); s += w) v.push(rmsOf(x, s, s + w));
  return median(v);
};

/** Crank rpm the synthesizer plays at audio time t (it ramps to each frame's rpm over one frame). */
function rpmAt(c, t) {
  const fr = c.frames;
  const u = clamp(t * 60 - 1, 0, fr.length - 1);
  const i = Math.floor(u);
  const j = Math.min(fr.length - 1, i + 1);
  return fr[i].rpm + (fr[j].rpm - fr[i].rpm) * (u - i);
}

/** Click detector of e2e/engine_render.mjs: 50 ms windows, max |2nd difference| vs its RMS. */
function clicks(x) {
  const win = Math.round(0.05 * SR);
  const out = [];
  let prevM = 0;
  let prevD = 0;
  for (let s = 0; s + win <= x.length; s += win) {
    let dsum = 0;
    let dmax = 0;
    for (let i = s; i < s + win; i++) {
      const d = x[i] - prevM;
      const dd = Math.abs(d - prevD);
      prevM = x[i];
      prevD = d;
      dsum += dd * dd;
      if (dd > dmax) dmax = dd;
    }
    const drms = Math.sqrt(dsum / win);
    if (drms > 1e-6 && dmax / drms > 12 && dmax > 0.02) out.push(+(s / SR).toFixed(2));
  }
  return out;
}

function wav16(x) {
  const data = Buffer.alloc(x.length * 2);
  for (let i = 0; i < x.length; i++) data.writeInt16LE(Math.round(clamp(x[i], -1, 1) * 32767), 2 * i);
  const h = Buffer.alloc(44);
  h.write("RIFF", 0);
  h.writeUInt32LE(36 + data.length, 4);
  h.write("WAVEfmt ", 8);
  h.writeUInt32LE(16, 16);
  h.writeUInt16LE(1, 20);
  h.writeUInt16LE(1, 22);
  h.writeUInt32LE(SR, 24);
  h.writeUInt32LE(SR * 2, 28);
  h.writeUInt16LE(2, 32);
  h.writeUInt16LE(16, 34);
  h.write("data", 36);
  h.writeUInt32LE(data.length, 40);
  return Buffer.concat([h, data]);
}

/** Log-frequency spectrogram (30 Hz – 8 kHz) as rows of dB values, for the PNG. */
function spectrogram(x, cols = 600, rows = 240) {
  const N = 4096;
  const hop = Math.max(1, Math.floor((x.length - N) / cols));
  const fLo = 30;
  const fHi = 8000;
  const out = [];
  for (let c = 0; c < cols; c++) {
    const s0 = c * hop;
    const re = new Float64Array(N);
    const im = new Float64Array(N);
    for (let i = 0; i < N; i++) re[i] = (x[s0 + i] ?? 0) * (0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (N - 1)));
    fft(re, im);
    const col = new Float64Array(rows);
    for (let r = 0; r < rows; r++) {
      const f = fLo * Math.pow(fHi / fLo, r / (rows - 1));
      const k = Math.min(N / 2 - 1, Math.round((f * N) / SR));
      col[r] = 10 * Math.log10(re[k] * re[k] + im[k] * im[k] + 1e-20);
    }
    out.push(Array.from(col));
  }
  return { cols: out, fLo, fHi, seconds: x.length / SR };
}

// ----- browser ----------------------------------------------------------------------------------

const PORT = 5196;
const URL = `http://localhost:${PORT}/`;
const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
async function waitForServer() {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(URL + "data/audio/audio_config.json")).ok) return;
    } catch {}
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error("vite did not start");
}

const bundle = await build({
  stdin: {
    contents: `export * from "./src/audio/voices.ts"; export * from "./src/audio/engine_host.ts";`,
    resolveDir: root,
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
  args: [
    "--autoplay-policy=no-user-gesture-required",
    "--use-gl=angle",
    "--use-angle=swiftshader",
    "--enable-unsafe-swiftshader",
    "--ignore-gpu-blocklist",
  ],
});
const page = await browser.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && !m.text().startsWith("Failed to load resource") && errors.push(m.text()));
await page.goto(URL + "data/audio/audio_config.json");
await page.setContent(`<html><head><base href="${URL}"></head><body><canvas id="c"></canvas></body></html>`);
await page.addScriptTag({ content: bundle.outputFiles[0].text });

await page.evaluate((SR) => {
  /**
   * Render cases at the same time (one entity each, all at the car's position, chase listener),
   * Engine bus only. `interior`: the focus perspective for every case. Returns mono samples as
   * base64 Float32 plus the worklet counters per entity.
   */
  window.renderCases = async (lib, cases, opts) => {
    const seconds = Math.max(...cases.map((c) => c.frames.length / 60)) + 0.3;
    const ctx = new OfflineAudioContext(2, Math.ceil(seconds * SR), SR);
    const mixer = new WPA.AudioMixer(ctx);
    const pool = new WPA.VoicePool(ctx, mixer, lib);
    pool.source = "procedural";
    if (!opts.noHost) pool.engineHost = await WPA.EngineSynthHost.create(ctx);
    WPA.setListener(ctx, [0, -6, 2.3], [0, 1, 0], [0, 0, 1]);
    const q = 128 / SR;
    const n = Math.max(...cases.map((c) => c.frames.length));
    for (let k = 0; k < n; k++) {
      const t = Math.max(q, Math.round(k / 60 / q) * q);
      ctx.suspend(t).then(() => {
        cases.forEach((c, i) => {
          const f = c.frames[k];
          if (!f) return;
          const entity = 1 + i;
          for (const cmd of f.commands) {
            if ("Play" in cmd) {
              const p = cmd.Play;
              if (p.bus !== "Engine") continue;
              pool.play({ entity, slot: p.voice.slot }, p.bus, p.pack, p.cue, p.looped, p.params, p.synth);
            } else if ("Set" in cmd) pool.set({ entity, slot: cmd.Set.voice.slot }, cmd.Set.params);
            else if ("Stop" in cmd) pool.stop({ entity, slot: cmd.Stop.voice.slot }, cmd.Stop.fade_s);
          }
          if (opts.interior && pool.engineHost) pool.engineHost.setInterior(entity, 1);
        });
        pool.collect();
        ctx.resume();
      });
    }
    const t0 = performance.now();
    const buf = await ctx.startRendering();
    const wallMs = performance.now() - t0;
    const counters = pool.engineHost ? await pool.engineHost.counters() : [];
    const L = buf.getChannelData(0);
    const R = buf.getChannelData(1);
    const m = new Float32Array(L.length);
    for (let i = 0; i < L.length; i++) m[i] = 0.5 * (L[i] + R[i]);
    let bin = "";
    const bytes = new Uint8Array(m.buffer);
    for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return { pcm: btoa(bin), wallMs, blocks: Math.ceil(L.length / 128), counters, refused: pool.engineHost?.stats.refused ?? 0 };
  };

  /**
   * Seven modelled engines, then every one re-played at once (what a director reset or a
   * sound-source switch does) twice: no refused slot, seven engines playing.
   */
  window.slotChurn = async (lib, cs) => {
    const ctx = new OfflineAudioContext(2, SR, SR);
    const pool = new WPA.VoicePool(ctx, new WPA.AudioMixer(ctx), lib);
    pool.engineHost = await WPA.EngineSynthHost.create(ctx);
    for (let round = 0; round < 3; round++) {
      cs.forEach((c, i) => {
        const p = c.frames[0].commands.find((k) => "Play" in k && k.Play.cue === "engine/physical").Play;
        pool.play({ entity: 1 + i, slot: p.voice.slot }, p.bus, p.pack, p.cue, p.looped, p.params, p.synth);
      });
      pool.collect();
    }
    return { refused: pool.engineHost.stats.refused, playing: pool.engineHost.playing };
  };

  /** Draw a spectrogram (log frequency) with the expected firing frequency; returns a JPEG data URL. */
  window.drawSpectrogram = (sg, fire, title) => {
    const W = sg.cols.length;
    const H = sg.cols[0].length;
    const c = document.getElementById("c");
    c.width = W;
    c.height = H + 18;
    const g = c.getContext("2d");
    const img = g.createImageData(W, H);
    let mx = -Infinity;
    for (const col of sg.cols) for (const v of col) mx = Math.max(mx, v);
    for (let x = 0; x < W; x++) {
      for (let y = 0; y < H; y++) {
        const v = Math.max(0, Math.min(1, (sg.cols[x][H - 1 - y] - (mx - 70)) / 70));
        const i = 4 * (y * W + x);
        img.data[i] = 255 * Math.min(1, v * 1.6);
        img.data[i + 1] = 255 * Math.max(0, v * 1.6 - 0.6);
        img.data[i + 2] = 255 * (0.25 + 0.5 * v) * (1 - v);
        img.data[i + 3] = 255;
      }
    }
    g.fillStyle = "#000";
    g.fillRect(0, 0, W, H + 18);
    g.putImageData(img, 0, 18);
    // Expected firing frequency (dashed white).
    g.strokeStyle = "rgba(255,255,255,0.55)";
    g.setLineDash([3, 3]);
    g.beginPath();
    fire.forEach(([t, f], i) => {
      const x = (t / sg.seconds) * W;
      const y = 18 + H - 1 - (Math.log(f / sg.fLo) / Math.log(sg.fHi / sg.fLo)) * (H - 1);
      if (i === 0) g.moveTo(x, y);
      else g.lineTo(x, y);
    });
    g.stroke();
    g.fillStyle = "#ddd";
    g.font = "12px sans-serif";
    g.fillText(title, 4, 13);
    return c.toDataURL("image/jpeg", 0.85);
  };
}, SR);

const decode = (b64) => {
  const buf = Buffer.from(b64, "base64");
  return new Float32Array(buf.buffer, buf.byteOffset, buf.length / 4);
};

const physical = trace.cases.filter((c) => c.mode === "Physical");
const layers = trace.cases.filter((c) => c.mode === "Layers");
const mediaDir = join(repo, "docs/media/audio");
if (media) mkdirSync(mediaDir, { recursive: true });
const report = [];
const describe = (c) => `${c.car} (${c.cylinders}-cyl ${c.layout} ${c.firing}${c.turbo ? " turbo" : ""})`;

// ----- 1 + 2: every sweep -------------------------------------------------------------------------
const rendered = {};
let sportyPops = 0;
for (const c of physical) {
  const r = await page.evaluate(async ({ lib, c }) => window.renderCases(lib, [c], {}), { lib: trace.library, c });
  const x = decode(r.pcm);
  rendered[`${c.car}/${c.sweep}`] = { x, counters: r.counters[0] ?? {} };
}
const layerLevel = {};
for (const c of layers) {
  const r = await page.evaluate(async ({ lib, c }) => window.renderCases(lib, [c], { noHost: true }), { lib: trace.library, c });
  const x = decode(r.pcm);
  layerLevel[c.car] = rmsOf(x, Math.round(0.2 * SR), Math.round(2.6 * SR));
}

for (const c of physical) {
  const { x, counters } = rendered[`${c.car}/${c.sweep}`];
  const name = `${c.car} ${c.sweep}`;
  const row = { car: c.car, sweep: c.sweep, what: describe(c), counters };
  report.push(row);
  const fireOrder = c.layout === "Rotary" ? c.cylinders : c.cylinders / 2;
  // No NaN, no subnormals, no clicks.
  let bad = 0;
  let sub = 0;
  let peak = 0;
  for (const v of x) {
    if (!Number.isFinite(v)) bad++;
    else if (v !== 0 && Math.abs(v) < 1.1754944e-38) sub++;
    peak = Math.max(peak, Math.abs(v));
  }
  // Afterfire pops (overrun, limiter, misfires) are wanted bangs, not clicks: the windows a pop
  // starts in (worklet onsets, ±1 window for the slot's start offset) are left out.
  const popAt = (counters.popAt ?? []).map((n) => n / SR);
  const nearPop = (t) => popAt.some((p) => t > p - 0.06 && t < p + 0.06);
  const allClicks = clicks(x);
  const cl = allClicks.filter((t) => !nearPop(t));
  row.peak = peak;
  row.clicks = cl.length;
  row.popWindows = allClicks.length - cl.length;
  check(bad === 0 && sub === 0, `${name}: finite, no subnormal samples (${bad} NaN, ${sub} subnormal)`);
  check(cl.length === 0, `${name}: no clicks outside pops${cl.length ? ` (at ${cl.slice(0, 8).join(" ")} s)` : ""}`);
  check(peak > 0.01 && peak <= 1, `${name}: audible and unclipped (peak ${peak.toFixed(3)})`);

  if (c.sweep === "steps") {
    // Pitch on each plateau; harmonic structure on the 40 % plateau.
    const per = 3.0 / 8;
    const errs = [];
    for (let i = 0; i < 8; i++) {
      const s0 = Math.round((i * per + 0.12) * SR);
      const s1 = Math.round(((i + 1) * per - 0.02) * SR);
      const rpm = c.frames[Math.round((i * per + 0.2) * 60)].rpm;
      const f0 = (rpm / 60) * fireOrder;
      const sp = spectrum(x, s0, s1);
      const pk = peakIn(sp, f0 * 0.94, f0 * 1.06);
      const prom = pk.db - medianIn(sp, f0 * 0.6, f0 * 1.4);
      errs.push({ rpm, f0, f: pk.f, err: Math.abs(pk.f - f0) / f0, prom });
    }
    const worst = errs.reduce((a, b) => (b.err > a.err ? b : a));
    row.pitch = errs;
    check(worst.err < 0.01, `${name}: firing harmonic within 1 % of rpm on all 8 plateaus (worst ${(100 * worst.err).toFixed(2)} % at ${worst.rpm.toFixed(0)} rpm)`);
    const weak = errs.filter((e) => e.prom < 10);
    check(weak.length === 0, `${name}: firing harmonic ≥ 10 dB above its band (min ${Math.min(...errs.map((e) => e.prom)).toFixed(1)} dB)`);
    // Orders at the 40 % plateau (index 2).
    const i = 2;
    const rpm = c.frames[Math.round((i * per + 0.2) * 60)].rpm;
    const crank = rpm / 60;
    const sp = spectrum(x, Math.round((i * per + 0.06) * SR), Math.round(((i + 1) * per - 0.01) * SR));
    const lvl = (o) => peakIn(sp, crank * o * 0.985, crank * o * 1.015).db;
    const fire = lvl(fireOrder);
    const orders = [];
    for (let k = 1; k <= 4 * fireOrder; k++) orders.push([k / 2, +(lvl(k / 2) - fire).toFixed(1)]);
    row.orders = orders;
    const half = Math.max(...orders.filter(([o]) => o % 1 !== 0 && o < 2 * fireOrder).map(([, d]) => d));
    row.halfOrders = half;
    const below = Math.max(...orders.filter(([o]) => o < fireOrder).map(([, d]) => d));
    if (c.firing === "CrossPlane") {
      check(half >= -12, `${name}: cross-plane V8 burble — half orders ${half.toFixed(1)} dB vs the firing order (≥ −12)`);
    } else {
      check(half <= -15, `${name}: even firing — half orders ${half.toFixed(1)} dB vs the firing order (≤ −15)`);
    }
    if (c.layout === "Inline") {
      check(below < 0, `${name}: the firing order (${fireOrder}) leads every lower order (max below ${below.toFixed(1)} dB)`);
    }
  }
  if (c.sweep === "wot" || c.sweep === "damaged") {
    // Pitch along the sweep (85 ms windows, 0.25 s … 2.5 s), crank lag of one frame included.
    const N = 4096;
    const errs = [];
    for (let s0 = Math.round(0.25 * SR); s0 + N < 2.5 * SR; s0 += N / 2) {
      const tc = (s0 + N / 2) / SR;
      const f0 = (rpmAt(c, tc) / 60) * fireOrder;
      const pk = peakIn(spectrum(x, s0, s0 + N), f0 * 0.93, f0 * 1.07);
      errs.push(Math.abs(pk.f - f0) / f0);
    }
    row.sweepPitchMedian = median(errs);
    row.sweepPitchP90 = quantile(errs, 0.9);
    if (c.sweep === "wot") {
      check(median(errs) < 0.01, `${name}: pitch tracks the rpm sweep (median error ${(100 * median(errs)).toFixed(2)} %, p90 ${(100 * quantile(errs, 0.9)).toFixed(2)} %)`);
      const lvl = rmsOf(x, Math.round(0.2 * SR), Math.round(2.6 * SR));
      const ref = layerLevel[c.car];
      row.levelVsLayersDb = dB(lvl) - dB(ref);
      check(Math.abs(dB(lvl) - dB(ref)) <= 6, `${name}: level ${dB(lvl).toFixed(1)} dBFS within 6 dB of the procedural layers (${dB(ref).toFixed(1)})`);
      check((counters.cuts ?? 0) > 0, `${name}: rev limiter cuts fuel on the limiter (${counters.cuts ?? 0} cut events)`);
    } else {
      check((counters.misfires ?? 0) > 5, `${name}: damaged engine misfires (${counters.misfires ?? 0} of ${counters.events ?? 0} events)`);
    }
  }
  if (c.sweep === "lift") {
    const sporty = ["Sport", "Muscle", "Exotic"].includes(c.class);
    row.pops = counters.pops ?? 0;
    if (sporty) {
      // Pops are random (≈ 3–6 expected per lift); each sporty car crackles, ≥ 8 in total below.
      sportyPops += counters.pops ?? 0;
      check((counters.pops ?? 0) >= 1, `${name}: overrun crackle after the lift (${counters.pops ?? 0} pops)`);
      // ...and is heard: the loudest 10 ms within 30 ms of a pop's onset stand out of the
      // overrun body (the pops used to die on their first sample: counted, never audible).
      const bed = bodyOf(x, 1.6, 2.9);
      const rise = (counters.popAt ?? [])
        .map((n) => n / SR)
        .filter((t) => t > 0.8 && t + 0.04 < x.length / SR)
        .map((t) => {
          let best = 0;
          for (let a = Math.round(t * SR); a < Math.round((t + 0.03) * SR); a += Math.round(0.002 * SR)) best = Math.max(best, rmsOf(x, a, a + Math.round(0.01 * SR)));
          return dB(best) - dB(bed);
        });
      if (rise.length) {
        check(median(rise) >= 7, `${name}: crackle is audible (median ${median(rise).toFixed(1)} dB over the overrun body, ${rise.length} pops)`);
      }
    } else {
      row.pops = counters.pops ?? 0;
    }
    if (c.turbo) check((counters.blowoffs ?? 0) === 1, `${name}: one blow-off on the lift (${counters.blowoffs ?? 0})`);
    // Load drop: the overrun note is quieter than the WOT part.
    const on = bodyOf(x, 0.2, 0.65);
    const off = bodyOf(x, 1.6, 2.9);
    row.liftDropDb = dB(on) - dB(off);
    check(dB(on) - dB(off) > 4, `${name}: lift-off drops the level (${(dB(on) - dB(off)).toFixed(1)} dB)`);
  }
  if (c.sweep === "shift") {
    // The cut is heard: the exhaust note (< 800 Hz; a turbo's blow-off hiss fills the highs)
    // dips during the shift.
    const lp = lowpass(x, 800);
    const pre = bodyOf(lp, 0.9, 1.15);
    const cut = bodyOf(lp, 1.25, 1.42);
    row.shiftDipDb = dB(pre) - dB(cut);
    check(dB(pre) - dB(cut) > 3, `${name}: shift cut dips the level (${(dB(pre) - dB(cut)).toFixed(1)} dB)`);
  }

  if (media && ["wot", "lift", "shift", "damaged"].includes(c.sweep)) {
    const base = `engine_${c.car}_${c.sweep}`;
    writeFileSync(join(mediaDir, `${base}.wav`), wav16(x.subarray(0, Math.round(3.0 * SR))));
    const sg = spectrogram(x.subarray(0, Math.round(3.0 * SR)));
    const fire = c.frames.map((f) => [f.t + 1 / 60, Math.max(31, f.firing_hz)]);
    const png = await page.evaluate(({ sg, fire, title }) => window.drawSpectrogram(sg, fire, title), {
      sg,
      fire,
      title: `${describe(c)} — ${c.sweep} (dashed: firing frequency)`,
    });
    writeFileSync(join(mediaDir, `${base}.jpg`), Buffer.from(png.split(",")[1], "base64"));
    row.files = [`${base}.wav`, `${base}.jpg`];
  }
}

const sportyLifts = physical.filter((c) => c.sweep === "lift" && ["Sport", "Muscle", "Exotic"].includes(c.class)).length;
check(sportyPops >= 2 * sportyLifts, `overrun crackle: ${sportyPops} pops over ${sportyLifts} sporty lifts (≥ ${2 * sportyLifts})`);

// Interior vs exterior (cross-plane V8, WOT sweep).
{
  const c = physical.find((k) => k.car === "rampart_50" && k.sweep === "wot");
  const r = await page.evaluate(async ({ lib, c }) => window.renderCases(lib, [c], { interior: true }), { lib: trace.library, c });
  const xin = decode(r.pcm);
  const xout = rendered["rampart_50/wot"].x;
  const hiShare = (x) => {
    const sp = spectrum(x, Math.round(1.0 * SR), Math.round(2.4 * SR));
    let lo = 0;
    let hi = 0;
    for (let i = 1; i < sp.db.length; i++) {
      const p = Math.pow(10, sp.db[i] / 10);
      if (i * sp.hz < 1000) lo += p;
      else if (i * sp.hz < 8000) hi += p;
    }
    return 10 * Math.log10(hi / lo);
  };
  const dIn = hiShare(xin);
  const dOut = hiShare(xout);
  check(dOut - dIn > 6, `interior is darker than exterior: energy above 1 kHz ${dIn.toFixed(1)} dB vs ${dOut.toFixed(1)} dB (rel. below 1 kHz)`);
  const popIn = (r.counters[0]?.popAt ?? []).map((n) => n / SR);
  const clIn = clicks(xin).filter((t) => !popIn.some((p) => t > p - 0.06 && t < p + 0.06));
  check(clIn.length === 0, `interior render: no clicks outside pops${clIn.length ? ` (at ${clIn.slice(0, 8).join(" ")} s)` : ""}`);
  if (media) {
    writeFileSync(join(mediaDir, "engine_rampart_50_wot_interior.wav"), wav16(xin.subarray(0, Math.round(3.0 * SR))));
  }
  report.push({ car: "rampart_50", sweep: "wot (interior)", what: "interior perspective", hiShareInterior: dIn, hiShareExterior: dOut, files: ["engine_rampart_50_wot_interior.wav"] });
}

// ----- 2b: the synthesizer alone (Node, no Web Audio) -----------------------------------------------
// Playtest "engine sounds are bad" (docs/integration/audio.md): the old voice drove V8s / sixes
// into its tanh soft limit at full throttle, swept firing harmonics through deep pipe-comb notches
// (level holes while revving), buried the I4's firing order ~10 dB under its harmonics and was
// perfectly periodic (a buzzer). Each car's EngineDsp, at its own spec, steady frame state:
{
  const mod = await build({ entryPoints: [join(root, "src/audio/engine_worklet.ts")], bundle: true, write: false, format: "esm", target: "es2022" });
  const { engineWorkletMain } = await import("data:text/javascript;base64," + Buffer.from(mod.outputFiles[0].text).toString("base64"));
  const { EngineDsp } = engineWorkletMain(null);
  const specOf = (car) => {
    for (const f of physical.find((k) => k.car === car && k.sweep === "wot").frames)
      for (const cmd of f.commands) if ("Play" in cmd && cmd.Play.cue === "engine/physical") return cmd.Play.synth;
    throw new Error(`no engine/physical play for ${car}`);
  };
  /** Render `secs` of a linear rpm ramp r0 → r1 at full throttle in 3rd (frames at 60 Hz). */
  const dsp = (spec, r0, r1, secs) => {
    const d = new EngineDsp(SR, spec);
    const n = Math.round(secs * SR);
    const y = new Float32Array(n);
    const F = new Float32Array(12);
    for (let s = 0; s < n; s += 800) {
      F.set([1, r0 + ((r1 - r0) * s) / n, 1, 1, 1, 3, 20, 0, 1, 0, 1, 0]);
      d.setFrame(F, 800);
      for (let j = s; j < Math.min(n, s + 800); j += 128) d.render(y, j, Math.min(128, n - j, s + 800 - j));
    }
    return y;
  };
  const flat = [];
  for (const car of [...new Set(physical.map((c) => c.car))]) {
    const spec = specOf(car);
    const idle = spec.idle_rpm || 800;
    const red = spec.redline_rpm || 6500;
    const cyl = spec.n || 4;
    // Slow WOT pull idle → redline.
    const secs = 8;
    const y = dsp(spec, idle, red, secs);
    let knee = 0;
    for (const v of y) if (Math.abs(v) > 0.8) knee++;
    check(knee === 0, `${car}: full-throttle pull stays out of the synth's soft limit (${knee} samples > 0.8)`);
    // Level along the pull (100 ms windows) detrended against log rpm, and the firing order's
    // level against the strongest partial.
    const lv = [];
    const fund = [];
    const win = Math.round(0.1 * SR);
    for (let s0 = win; s0 + win <= y.length; s0 += win) {
      const rpm = idle + ((red - idle) * (s0 + win / 2)) / y.length;
      lv.push([Math.log(rpm), 20 * Math.log10(rmsOf(y, s0, s0 + win) + 1e-12)]);
      const sp = spectrum(y, s0, s0 + win, 1 << 15);
      const f = (rpm / 60) * (cyl / 2);
      const h = [1, 2, 3, 4, 5, 6].map((m) => peakIn(sp, m * f * 0.96, m * f * 1.04).db);
      fund.push(h[0] - Math.max(...h));
    }
    const mx = lv.reduce((a, v) => a + v[0], 0) / lv.length;
    const my = lv.reduce((a, v) => a + v[1], 0) / lv.length;
    const k = lv.reduce((a, v) => a + (v[0] - mx) * (v[1] - my), 0) / lv.reduce((a, v) => a + (v[0] - mx) ** 2, 0);
    const resid = lv.map((v) => v[1] - (my + k * (v[0] - mx)));
    const sd = Math.sqrt(resid.reduce((a, v) => a + v * v, 0) / resid.length);
    check(sd <= 1.5, `${car}: WOT pull level follows the rpm without comb holes (detrended ripple ${sd.toFixed(2)} dB ≤ 1.5; was up to 2.8)`);
    check(median(fund) >= -7, `${car}: firing order within 7 dB of the strongest partial along the pull (median ${median(fund).toFixed(1)} dB; the I4 was -10)`);
    // Spectral flatness 200 Hz-4 kHz at 40 % of redline, WOT (recordings: -10 to -23 dB).
    const z = dsp(spec, 0.4 * red, 0.4 * red, 1.5);
    const sp = spectrum(z, Math.round(0.3 * SR), z.length);
    let lg = 0;
    let ar = 0;
    let cnt = 0;
    for (let i = Math.ceil(200 / sp.hz); i <= Math.floor(4000 / sp.hz); i++) {
      lg += (sp.db[i] / 10) * Math.LN10;
      ar += 10 ** (sp.db[i] / 10);
      cnt++;
    }
    flat.push(10 * Math.log10(Math.exp(lg / cnt) / (ar / cnt)));
    if (verbose) console.log(`  ${car}: ripple ${sd.toFixed(2)} dB, firing order ${median(fund).toFixed(1)} dB, flatness ${flat[flat.length - 1].toFixed(1)} dB`);
  }
  check(median(flat) >= -22, `not a buzzer: median spectral flatness ${median(flat).toFixed(1)} dB ≥ -22 (was ≈ -30; recordings -10 to -23)`);
}

// ----- 3: budget ----------------------------------------------------------------------------------
{
  // Player + 6 nearby: seven modelled engines at once (the sweep cars and sweeps, mixed).
  const seven = [
    ["rampart_50", "wot"],
    ["debug_sedan", "wot"],
    ["sperber_32", "shift"],
    ["lucciola_350", "wot"],
    ["shiro_turbo", "lift"],
    ["rampart_50", "shift"],
    ["shiro_turbo", "wot"],
  ].map(([car, sweep]) => physical.find((k) => k.car === car && k.sweep === sweep));
  const runs = [];
  for (let rep = 0; rep < 3; rep++) {
    const full = await page.evaluate(async ({ lib, cs }) => window.renderCases(lib, cs, {}), { lib: trace.library, cs: seven });
    const empty = await page.evaluate(async ({ lib, cs }) => window.renderCases(lib, cs, { noHost: true }), {
      lib: trace.library,
      cs: seven.map((c) => ({ ...c, frames: c.frames.map((f) => ({ ...f, commands: [] })) })),
    });
    runs.push({ ms: (full.wallMs - empty.wallMs) / full.blocks, refused: full.refused });
  }
  const best = Math.min(...runs.map((r) => r.ms));
  const msPerBlock = median(runs.map((r) => r.ms));
  check(runs.every((r) => r.refused === 0), "seven engines get seven worklet slots");
  const churn = await page.evaluate(async ({ lib, cs }) => window.slotChurn(lib, cs), { lib: trace.library, cs: seven });
  check(churn.refused === 0 && churn.playing === 7, `all seven engines re-played twice at once (reset / source switch): ${churn.playing} playing, ${churn.refused} refused`);
  check(msPerBlock <= 1.5, `budget: 7 modelled engines cost ${msPerBlock.toFixed(3)} ms per 128-frame block (median of 3, best ${best.toFixed(3)}; budget 1.5 ms)`);
  report.push({ car: "7 engines", sweep: "budget", what: "player + 6 nearby", msPerBlock, best });
}

// ----- 4: in the game page ------------------------------------------------------------------------
if (runGame) {
  const game = await browser.newPage({ viewport: { width: 1200, height: 760 } });
  const gerr = [];
  game.on("pageerror", (e) => gerr.push(String(e)));
  game.on("console", (m) => m.type() === "error" && gerr.push(m.text()));
  await game.goto(URL);
  await game.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
  await game.keyboard.press("KeyX");
  await game.waitForFunction(() => window.__wpAudio?.running(), null, { timeout: 10000 });
  await game.waitForFunction(() => window.__wpAudio.engineSynth().state !== "loading", null, { timeout: 10000 });
  const st = await game.evaluate(() => window.__wpAudio.engineSynth());
  check(st.state === "ready", `game: engine synthesizer loaded (${st.state})`);
  await game.keyboard.down("KeyW");
  await game.waitForTimeout(2000);
  const playing = await game.evaluate(() => ({ s: window.__wpAudio.engineSynth(), c: { ...window.__wpAudio.cueCounts }, v: window.__wpAudio.engineVoicing() }));
  check(playing.v === "modelled" && playing.s.playing >= 1 && (playing.c["engine/physical"] ?? 0) >= 1, `game: the player's engine is modelled (${playing.s.playing} playing, voicing ${playing.v})`);
  await game.evaluate(() => window.__wpAudio.setEngineVoicing("layers"));
  await game.waitForTimeout(800);
  const lay = await game.evaluate(() => ({ s: window.__wpAudio.engineSynth(), c: { ...window.__wpAudio.cueCounts }, loops: window.__wpAudio.loopCounts() }));
  const layerPlays = Object.entries(lay.c).filter(([k]) => k.startsWith("engine/") && k !== "engine/physical").reduce((a, [, v]) => a + v, 0);
  check(lay.s.playing === 0 && layerPlays > 0, `game: "layers" moves the engine back to layers (${lay.s.playing} modelled, ${layerPlays} layer plays)`);
  check(!lay.loops || lay.loops.backend === lay.loops.director, `game: no stale voices after the switch (${JSON.stringify(lay.loops)})`);
  await game.evaluate(() => window.__wpAudio.setEngineVoicing("modelled"));
  await game.waitForTimeout(800);
  const back = await game.evaluate(() => window.__wpAudio.engineSynth());
  check(back.playing >= 1, `game: back to modelled (${back.playing} playing)`);
  await game.keyboard.up("KeyW");
  check(gerr.length === 0, `game: no page errors${gerr.length ? `: ${gerr.slice(0, 3).join(" | ")}` : ""}`);
  await game.close();
}

check(errors.length === 0, `no page errors${errors.length ? `: ${errors.slice(0, 3).join(" | ")}` : ""}`);
await browser.close();

// ----- report -------------------------------------------------------------------------------------
if (media) {
  const L = [];
  L.push("# Engine synthesizer — offline renders and spectra");
  L.push("");
  L.push("Generated by `node apps/testbed-web/e2e/audio_engine.mjs` (owner row 73). Each sweep is the");
  L.push("director's command stream (`cargo run -p wp_audio --example engine_sweeps`) rendered through the");
  L.push("testbed's real `VoicePool` and engine AudioWorklet in an `OfflineAudioContext` (48 kHz, chase");
  L.push("camera 6 m behind, exterior unless noted). Spectrograms: log frequency 30 Hz – 8 kHz, dashed line =");
  L.push("expected firing frequency `rpm / 60 × cylinders / 2`.");
  L.push("");
  L.push("Sweeps (3 s): **wot** idle → redline at full throttle in 2nd, last 0.4 s on the rev limiter;");
  L.push("**lift** full throttle near 80 % redline then a lift (overrun, crackle, blow-off on turbo cars);");
  L.push("**shift** 2nd-gear pull, 0.25 s clutch shift (cut + rpm drop), 3rd-gear pull; **damaged** WOT at");
  L.push("35 % engine health (misfires).");
  L.push("");
  L.push("| car | sweep | files | pitch (steps: worst plateau / sweep: median) | half orders vs firing | notes |");
  L.push("|---|---|---|---|---|---|");
  for (const r of report) {
    const steps = report.find((q) => q.car === r.car && q.sweep === "steps");
    const pitch =
      r.sweep === "wot" || r.sweep === "damaged"
        ? `${(100 * (r.sweepPitchMedian ?? 0)).toFixed(2)} % (p90 ${(100 * (r.sweepPitchP90 ?? 0)).toFixed(2)} %)`
        : r.sweep === "steps"
          ? `${(100 * Math.max(...r.pitch.map((e) => e.err))).toFixed(2)} %`
          : "";
    const half = r.sweep === "steps" ? `${r.halfOrders.toFixed(1)} dB` : r.sweep === "wot" && steps ? `(steps: ${steps.halfOrders.toFixed(1)} dB)` : "";
    const notes = [];
    if (r.levelVsLayersDb !== undefined) notes.push(`level vs layers ${r.levelVsLayersDb.toFixed(1)} dB`);
    if (r.counters?.cuts) notes.push(`${r.counters.cuts} limiter cuts`);
    if (r.counters?.misfires) notes.push(`${r.counters.misfires}/${r.counters.events} misfires`);
    if (r.pops !== undefined) notes.push(`${r.pops} pops`);
    if (r.counters?.blowoffs) notes.push(`${r.counters.blowoffs} blow-off`);
    if (r.liftDropDb !== undefined) notes.push(`lift drop ${r.liftDropDb.toFixed(1)} dB`);
    if (r.shiftDipDb !== undefined) notes.push(`shift dip ${r.shiftDipDb.toFixed(1)} dB`);
    if (r.hiShareInterior !== undefined) notes.push(`> 1 kHz share ${r.hiShareInterior.toFixed(1)} dB inside vs ${r.hiShareExterior.toFixed(1)} dB outside`);
    if (r.msPerBlock !== undefined) notes.push(`${r.msPerBlock.toFixed(3)} ms / 128-frame block (best ${r.best.toFixed(3)})`);
    const files = (r.files ?? []).map((f) => `[${f.endsWith(".jpg") ? "spectrogram" : "wav"}](${f})`).join(" · ");
    L.push(`| ${r.what} | ${r.sweep} | ${files} | ${pitch} | ${half} | ${notes.join("; ")} |`);
  }
  L.push("");
  L.push("Engine orders at the 40 % redline plateau (dB relative to the firing order; crank order 1 =");
  L.push("rpm / 60):");
  L.push("");
  for (const r of report.filter((q) => q.sweep === "steps")) {
    L.push(`- **${r.what}**: ${r.orders.map(([o, d]) => `${o}: ${d}`).join(", ")}`);
  }
  L.push("");
  writeFileSync(join(mediaDir, "README.md"), L.join("\n"));
}
if (verbose) console.log(JSON.stringify(report, (k, v) => (k === "pitch" ? undefined : v), 1));
console.log(failures ? `\n${failures} check(s) FAILED` : "\nall engine synthesizer checks passed");
process.exit(failures ? 1 : 0);
