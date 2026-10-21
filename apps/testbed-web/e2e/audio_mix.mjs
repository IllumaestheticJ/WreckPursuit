// Master-mix headroom: realistic scenes through the whole testbed mixer (every bus, siren
// rack, scanner squelch / static, city ambience bed, engine worklet, master compressor and
// safety soft clip), rendered offline in headless Chromium and measured.
//
// Usage: node e2e/audio_mix.mjs [--trace trace.json] [--scenes chase,idle] [--wav <dir>]
//                               [--label <name>] [--solo] [--report <file.json>] [--no-check]
//                               [--src <dir>] [--master <file.json>]
//   trace: `cargo run --release -p wp_audio --example mix_trace -- trace.json` (generated when
//   omitted). Scenes: v8_wot (one V8 at WOT, chase camera 6 m), chase (player V8 at WOT, three
//   police units with sirens — two following, one oncoming pass-by — a cornering squeal, a big
//   hit, two pursuit radio lines), cruise (light part throttle), idle.
//   --wav writes <dir>/<label>_<scene>.wav (16-bit stereo, the master output);
//   --src <dir> bundles the audio backend from another copy of src/audio (before / after renders);
//   --solo also renders the chase once per bus with the other buses cut (balance through the
//   whole master chain); --master <file.json> replaces the trace's master-stage settings (tuning).
//
// Per scene, over the scene's analysis window: share of samples in the soft clip (|out| above
// the clip's linear range), sample peak and true peak (4× oversampled) in dBFS, short-term
// loudness (EBU R128 / BS.1770 K-weighted 3 s windows: max and median) and integrated loudness,
// the master compressor's gain reduction (DynamicsCompressorNode.reduction sampled every 60 Hz
// frame: median / p95 / max dB) and each bus's level before the master stage (K-weighted,
// relative to the Engine bus: the balance).
//
// Checks (regression for the gain staging, docs/integration/audio.md "Mix headroom"):
//   - chase: ≤ 0.05 % of samples in the soft clip, true peak ≤ −1 dBTP, compressor reduction
//     p95 ≤ 2.5 dB (not pumping) and max ≤ 9 dB (the big hit's transient; 4–6 dB measured, it
//     varies with the impact recording picked at random), short-term loudness max within
//     −18 … −12 LUFS;
//   - v8_wot: no samples in the soft clip, compressor reduction p95 ≤ 1 dB;
//   - cruise / idle: no soft clip, no compressor reduction above 0.5 dB, audible (≥ −40 LUFS);
//   - below threshold the master stage is unity gain (the compressor's automatic make-up gain is
//     compensated): a −30 dBFS sine passes within ±0.25 dB.
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, extname, join } from "node:path";
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
const flag = (name) => process.argv.includes(`--${name}`);
const wavDir = arg("wav", null);
const label = arg("label", "mix");
const reportPath = arg("report", null);
const solo = flag("solo");
const noCheck = flag("no-check");
let tracePath = arg("trace", null);
if (!tracePath) {
  tracePath = join(mkdtempSync(join(tmpdir(), "wp-mix-")), "mix.json");
  const r = spawnSync("cargo", ["run", "-q", "--release", "-p", "wp_audio", "--example", "mix_trace", "--", tracePath], {
    cwd: repo,
    stdio: "inherit",
  });
  if (r.status !== 0) throw new Error("mix_trace failed");
}
const trace = JSON.parse(readFileSync(tracePath, "utf8"));
// --master <file.json>: try other master-stage settings on the same trace (tuning).
const masterOverride = arg("master", null);
if (masterOverride) trace.library.config.master = JSON.parse(readFileSync(masterOverride, "utf8"));
const only = arg("scenes", null)?.split(",");
const scenes = trace.scenes.filter((s) => !only || only.includes(s.name));

let failures = 0;
const check = (ok, msg) => {
  console.log(`${ok ? "PASS" : "FAIL"}  ${msg}`);
  if (!ok) failures++;
};

const bundle = await build({
  stdin: {
    contents: `export * from "./voices"; export * from "./engine_host"; export { SirenRack } from "./siren_rack"; export { ScannerPlayer } from "./scanner";`,
    resolveDir: arg("src", join(root, "src/audio")),
    loader: "ts",
  },
  bundle: true,
  write: false,
  format: "iife",
  globalName: "WPA",
  target: "es2022",
});

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--autoplay-policy=no-user-gesture-required"],
});
const page = await browser.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
// Serve the repo (recorded packs under data/audio/packs) without a dev server.
// localhost: a secure context (AudioWorklet needs one).
const BASE = "http://localhost:59175/";
await page.route(BASE + "**", (route) => {
  const u = new URL(route.request().url());
  if (u.pathname === "/") return route.fulfill({ status: 200, contentType: "text/html", body: "<html><body></body></html>" });
  try {
    const body = readFileSync(join(repo, decodeURIComponent(u.pathname)));
    const ct = { ".ogg": "audio/ogg", ".json": "application/json", ".wav": "audio/wav" }[extname(u.pathname)] ?? "application/octet-stream";
    return route.fulfill({ status: 200, contentType: ct, body });
  } catch {
    return route.fulfill({ status: 404, body: "not found" });
  }
});
await page.goto(BASE);
await page.addScriptTag({ content: bundle.outputFiles[0].text });

const SR = 48000;
const BUS_NAMES = ["Engine", "World", "Police", "Ui", "Music"];

/** Render one scene; `soloBus` cuts every other bus from the master. Returns float channels. */
async function render(scene, soloBus) {
  return page.evaluate(
    async ({ lib, frames, SR, BUS_NAMES, soloBus }) => {
      const dur = frames.length / 60 + 0.5;
      // Channels: 0-1 master output, then each bus (stereo) where it enters the master stage.
      const nch = 2 + 2 * BUS_NAMES.length;
      const ctx = new OfflineAudioContext(nch, Math.ceil(dur * SR), SR);
      // Capture the master compressor whatever the mixer calls it.
      const comps = [];
      const mk = ctx.createDynamicsCompressor.bind(ctx);
      ctx.createDynamicsCompressor = () => {
        const c = mk();
        comps.push(c);
        return c;
      };
      const mixer = new WPA.AudioMixer(ctx, lib.config.master);
      const pool = new WPA.VoicePool(ctx, mixer, lib);
      pool.source = "recorded";
      pool.engineHost = await WPA.EngineSynthHost.create(ctx);
      const rack = new WPA.SirenRack(ctx, mixer, lib);
      pool.router = rack;
      const scanner = new WPA.ScannerPlayer(pool);
      scanner.tts = false;
      for (let i = 0; i < 600 && pool.sampleStatus().loading > 0; i++) await new Promise((r) => setTimeout(r, 50));
      const status = pool.sampleStatus();
      // Taps (the mixer's own routing to the destination stays as in the game).
      const merger = ctx.createChannelMerger(nch);
      merger.connect(ctx.destination);
      ctx.destination.channelInterpretation = "discrete";
      const tap = (node, ch) => {
        const sp = ctx.createChannelSplitter(2);
        node.connect(sp);
        sp.connect(merger, 0, ch);
        sp.connect(merger, 1, ch + 1);
      };
      mixer.out.disconnect();
      tap(mixer.out, 0);
      BUS_NAMES.forEach((b, k) => {
        const outNode = mixer.busOut ? mixer.busOut(b) : mixer.buses[b].duck;
        tap(outNode, 2 + 2 * k);
        if (soloBus && b !== soloBus) outNode.disconnect(mixer.master);
      });
      // City ambience bed, as the game starts it (panel default 0.5).
      const amb = Object.keys(lib.packs).find((p) => pool.has(p, "ambient/city_night"));
      if (amb) pool.playLocal(amb, "ambient/city_night", 0.5, "World");
      const q = 128 / SR;
      const reduction = [];
      const errs = [];
      frames.forEach((f, k) => {
        const t = Math.max(q, Math.round(f.t / q) * q);
        ctx.suspend(t).then(() => {
          try {
            if (comps[0]) reduction.push([t, comps[0].reduction]);
            if (f.listener) WPA.setListener(ctx, ...f.listener);
            rack.frame(f.siren);
            // Same order as the game's AudioSystem.apply (index.ts).
            for (const c of f.commands) {
              if (c.Play) {
                const p = c.Play;
                rack.noteVoice(p.voice, p.pack);
                pool.play(p.voice, p.bus, p.pack, p.cue, p.looped, rack.adjust(p.voice, p.params), p.synth);
              } else if (c.Set) pool.set(c.Set.voice, rack.adjust(c.Set.voice, c.Set.params));
              else if (c.Stop) {
                rack.forget(c.Stop.voice);
                pool.stop(c.Stop.voice, c.Stop.fade_s);
              } else if (c.Duck) mixer.duck(c.Duck.bus, c.Duck.gain, c.Duck.fade_s);
              else if (c.RadioSay) {
                const r = c.RadioSay;
                scanner.say(r.line, r.unit, r.text, r.priority, r.voice_pitch);
                rack.setRadio(true);
              } else if (c.RadioEnd) {
                scanner.end(c.RadioEnd.line, c.RadioEnd.interrupted);
                rack.setRadio(false);
              }
            }
            pool.collect();
          } catch (e) {
            errs.push(`frame ${k}: ${e && e.stack ? e.stack : e}`);
          } finally {
            ctx.resume();
          }
        });
      });
      const buf = await Promise.race([
        ctx.startRendering(),
        new Promise((_, rej) => setTimeout(() => rej(new Error(`render stalled at ${ctx.currentTime.toFixed(2)} s`)), 600000)),
      ]);
      const b64 = (a) => {
        const u = new Uint8Array(a.buffer, a.byteOffset, a.byteLength);
        let s = "";
        for (let i = 0; i < u.length; i += 0x8000) s += String.fromCharCode(...u.subarray(i, i + 0x8000));
        return btoa(s);
      };
      const chans = [];
      for (let c = 0; c < nch; c++) chans.push(b64(buf.getChannelData(c)));
      const comp = comps[0] ? { threshold: comps[0].threshold.value, knee: comps[0].knee.value, ratio: comps[0].ratio.value } : null;
      return { status, errs, chans, reduction, comp, master: mixer.master.gain.value };
    },
    { lib: trace.library, frames: scene.frames, SR, BUS_NAMES, soloBus },
  ).then((r) => ({ ...r, chans: r.chans.map((s) => new Float32Array(Buffer.from(s, "base64").buffer.slice(0))) }));
}

/** Static gain of the master stage at low level: a −30 dBFS 1 kHz sine into the master input. */
async function unityCheck() {
  return page.evaluate(
    async ({ lib, SR }) => {
      const ctx = new OfflineAudioContext(2, SR * 2, SR);
      const mixer = new WPA.AudioMixer(ctx, lib.config.master);
      const osc = ctx.createOscillator();
      osc.frequency.value = 1000;
      const g = ctx.createGain();
      const amp = 10 ** (-30 / 20);
      g.gain.value = amp / mixer.master.gain.value; // level at the master gain's output
      osc.connect(g).connect(mixer.master);
      osc.start();
      const buf = await ctx.startRendering();
      const x = buf.getChannelData(0);
      let peak = 0;
      for (let i = SR; i < x.length; i++) peak = Math.max(peak, Math.abs(x[i]));
      return { db: 20 * Math.log10(peak / amp), defaults: WPA.DEFAULT_MASTER ?? null };
    },
    { lib: trace.library, SR },
  );
}

// ---- measurements -------------------------------------------------------------------------

const db = (x) => (x > 0 ? 20 * Math.log10(x) : -Infinity);

/** BS.1770 K-weighting at 48 kHz (pre-filter shelf + RLB high-pass). */
function kWeight(x) {
  const stages = [
    [[1.53512485958697, -2.69169618940638, 1.19839281085285], [-1.69065929318241, 0.73248077421585]],
    [[1, -2, 1], [-1.99004745483398, 0.99007225036621]],
  ];
  let y = x;
  for (const [[b0, b1, b2], [a1, a2]] of stages) {
    const o = new Float32Array(y.length);
    let x1 = 0, x2 = 0, y1 = 0, y2 = 0;
    for (let i = 0; i < y.length; i++) {
      const v = b0 * y[i] + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
      x2 = x1; x1 = y[i]; y2 = y1; y1 = v;
      o[i] = v;
    }
    y = o;
  }
  return y;
}

/** Loudness of a stereo pair: short-term (3 s, 100 ms hop) max / median and integrated (gated). */
function loudness(L, R, i0, i1) {
  const kl = kWeight(L.subarray(i0, i1));
  const kr = kWeight(R.subarray(i0, i1));
  const blk = Math.round(0.1 * SR);
  const ms = [];
  for (let s = 0; s + blk <= kl.length; s += blk) {
    let a = 0;
    for (let i = s; i < s + blk; i++) a += kl[i] * kl[i] + kr[i] * kr[i];
    ms.push(a / blk);
  }
  const lufs = (m) => -0.691 + 10 * Math.log10(m + 1e-20);
  const st = [];
  for (let k = 29; k < ms.length; k++) {
    let a = 0;
    for (let j = k - 29; j <= k; j++) a += ms[j];
    st.push(lufs(a / 30));
  }
  // Integrated: 400 ms blocks, 75 % overlap, absolute (−70) and relative (−10 LU) gates.
  const blocks = [];
  for (let k = 3; k < ms.length; k++) blocks.push((ms[k - 3] + ms[k - 2] + ms[k - 1] + ms[k]) / 4);
  const abs = blocks.filter((m) => lufs(m) > -70);
  const mean = (a) => a.reduce((s, v) => s + v, 0) / Math.max(a.length, 1);
  const rel = lufs(mean(abs)) - 10;
  const integrated = lufs(mean(abs.filter((m) => lufs(m) > rel)));
  st.sort((a, b) => a - b);
  return { stMax: st.length ? st[st.length - 1] : -Infinity, stMed: st.length ? st[st.length >> 1] : -Infinity, integrated };
}

/** True peak: 4× oversampled (windowed-sinc, 32 taps per phase), dBFS. */
function truePeak(x, i0, i1) {
  const P = 4;
  const T = 32;
  const h = [];
  for (let p = 0; p < P; p++) {
    const row = [];
    for (let k = 0; k < T; k++) {
      const n = k - T / 2 + 1 - p / P;
      const sinc = n === 0 ? 1 : Math.sin(Math.PI * n) / (Math.PI * n);
      const w = 0.5 + 0.5 * Math.cos((Math.PI * n) / (T / 2));
      row.push(sinc * w);
    }
    h.push(row);
  }
  let peak = 0;
  for (let i = Math.max(i0, T); i < Math.min(i1, x.length - T); i++) {
    for (let p = 0; p < P; p++) {
      let a = 0;
      const row = h[p];
      for (let k = 0; k < T; k++) a += row[k] * x[i + k - T / 2 + 1];
      const v = Math.abs(a);
      if (v > peak) peak = v;
    }
  }
  return db(peak);
}

/** Linear range of the master soft clip (identity below it). */
const CLIP_KNEE = 0.7;

function measure(scene, r) {
  const [t0, t1] = scene.analyse;
  const i0 = Math.round(t0 * SR);
  const i1 = Math.min(Math.round(t1 * SR), r.chans[0].length);
  const [L, R] = r.chans;
  let inClip = 0;
  let peak = 0;
  for (let i = i0; i < i1; i++) {
    const a = Math.abs(L[i]);
    const b = Math.abs(R[i]);
    if (a > CLIP_KNEE) inClip++;
    if (b > CLIP_KNEE) inClip++;
    peak = Math.max(peak, a, b);
  }
  const tp = Math.max(truePeak(L, i0, i1), truePeak(R, i0, i1));
  const loud = loudness(L, R, i0, i1);
  // The compressor's meter starts at about −10 dB in silence and settles over ~1.5 s (a
  // Chromium metering artefact, no gain is applied): skip the first 2 s.
  const red = r.reduction.filter(([t]) => t >= Math.max(t0, 2) && t <= t1).map(([, v]) => -v).sort((a, b) => a - b);
  const pct = (p) => (red.length ? red[Math.min(red.length - 1, Math.floor(p * red.length))] : 0);
  const buses = {};
  const busPeaks = {};
  BUS_NAMES.forEach((b, k) => {
    const l = r.chans[2 + 2 * k];
    const rr = r.chans[3 + 2 * k];
    buses[b] = loudness(l, rr, i0, i1).integrated;
    let p = 0;
    for (let i = i0; i < i1; i++) p = Math.max(p, Math.abs(l[i]), Math.abs(rr[i]));
    busPeaks[b] = db(p * r.master);
  });
  return {
    clipPct: (100 * inClip) / (2 * (i1 - i0)),
    peakDb: db(peak),
    truePeakDb: tp,
    ...loud,
    reduction: { median: pct(0.5), p95: pct(0.95), max: red.length ? red[red.length - 1] : 0 },
    buses,
    busPeaks,
  };
}

function writeWav(path, L, R) {
  const n = L.length;
  const pcm = new Int16Array(n * 2);
  for (let i = 0; i < n; i++) {
    pcm[2 * i] = Math.max(-1, Math.min(1, L[i])) * 32767;
    pcm[2 * i + 1] = Math.max(-1, Math.min(1, R[i])) * 32767;
  }
  const data = Buffer.from(pcm.buffer);
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
  writeFileSync(path, Buffer.concat([h, data]));
}

const f1 = (x) => (Number.isFinite(x) ? x.toFixed(1) : String(x));
const report = { label, scenes: {} };
if (wavDir) mkdirSync(wavDir, { recursive: true });

const { db: unity, defaults } = await unityCheck();
report.unityDb = unity;
console.log(`master stage gain for a −30 dBFS sine: ${unity.toFixed(2)} dB`);

for (const scene of scenes) {
  const r = await render(scene, null);
  if (r.errs.length) console.log(r.errs.slice(0, 5).join("\n"));
  check(r.errs.length === 0 && r.status.failed.length === 0, `${scene.name}: rendered (${r.status.loaded} samples, ${r.errs.length} errors)`);
  const m = measure(scene, r);
  report.scenes[scene.name] = m;
  if (r.comp) report.comp = r.comp;
  const rel = Object.fromEntries(Object.entries(m.buses).map(([b, v]) => [b, f1(v - m.buses.Engine)]));
  console.log(
    `${scene.name}: soft clip ${m.clipPct.toFixed(3)} %, peak ${f1(m.peakDb)} dBFS, true peak ${f1(m.truePeakDb)} dBTP, ` +
      `short-term ${f1(m.stMax)} max / ${f1(m.stMed)} median LUFS, integrated ${f1(m.integrated)} LUFS, ` +
      `comp reduction median ${f1(m.reduction.median)} / p95 ${f1(m.reduction.p95)} / max ${f1(m.reduction.max)} dB`,
  );
  console.log(`  bus levels before the master (LUFS): ${BUS_NAMES.map((b) => `${b} ${f1(m.buses[b])}`).join(", ")}; vs Engine: ${JSON.stringify(rel)}`);
  console.log(`  bus peaks × master gain (dBFS): ${BUS_NAMES.map((b) => `${b} ${f1(m.busPeaks[b])}`).join(", ")}`);
  if (wavDir) writeWav(join(wavDir, `${label}_${scene.name}.wav`), r.chans[0], r.chans[1]);
  if (solo && scene.name === "chase") {
    m.solo = {};
    for (const b of BUS_NAMES) {
      const rs = await render(scene, b);
      const ms = measure(scene, rs);
      m.solo[b] = { integrated: ms.integrated, stMax: ms.stMax, truePeakDb: ms.truePeakDb, reductionP95: ms.reduction.p95 };
      if (wavDir && Number.isFinite(ms.integrated)) writeWav(join(wavDir, `${label}_chase_solo_${b}.wav`), rs.chans[0], rs.chans[1]);
    }
    console.log(`  chase, buses solo through the master (integrated LUFS): ${BUS_NAMES.map((b) => `${b} ${f1(m.solo[b].integrated)}`).join(", ")}`);
  }
}
await browser.close();
if (reportPath) writeFileSync(reportPath, JSON.stringify(report, null, 2));

if (!noCheck) {
  check(Math.abs(unity) <= 0.25, `master stage is unity below threshold (${unity.toFixed(2)} dB; compressor make-up compensated)`);
  // Same keys and values (the trace widens f32 to f64: compare numbers with a tolerance).
  const same = (a, b) =>
    typeof a === "number" && typeof b === "number"
      ? Math.abs(a - b) <= 1e-6 * Math.max(1, Math.abs(a))
      : a && b && typeof a === "object" && typeof b === "object"
        ? Object.keys(a).length === Object.keys(b).length && Object.keys(a).every((k) => same(a[k], b[k]))
        : a === b;
  check(same(defaults, trace.library.config.master), "voices.ts DEFAULT_MASTER matches audio_config.json master");
  const c = report.scenes.chase;
  if (c) {
    check(c.clipPct <= 0.05, `chase: ≤ 0.05 % of samples in the soft clip (${c.clipPct.toFixed(3)} %)`);
    check(c.truePeakDb <= -1, `chase: true peak ≤ −1 dBTP (${f1(c.truePeakDb)})`);
    check(c.reduction.p95 <= 2.5 && c.reduction.max <= 9, `chase: compressor reduction p95 ≤ 2.5 dB, max ≤ 9 dB (${f1(c.reduction.p95)} / ${f1(c.reduction.max)})`);
    check(c.stMax >= -18 && c.stMax <= -12, `chase: short-term loudness max in −18 … −12 LUFS (${f1(c.stMax)})`);
  }
  const v = report.scenes.v8_wot;
  if (v) {
    check(v.clipPct === 0, `v8_wot: no samples in the soft clip (${v.clipPct.toFixed(3)} %)`);
    check(v.reduction.p95 <= 1, `v8_wot: compressor reduction p95 ≤ 1 dB (${f1(v.reduction.p95)})`);
  }
  for (const n of ["cruise", "idle"]) {
    const s = report.scenes[n];
    if (!s) continue;
    check(s.clipPct === 0 && s.reduction.max <= 0.5, `${n}: no soft clip, no compression (${s.clipPct.toFixed(3)} %, max ${f1(s.reduction.max)} dB)`);
    check(s.integrated >= -40, `${n}: audible (${f1(s.integrated)} LUFS)`);
  }
}
check(errors.length === 0, `no page errors (${errors.slice(0, 3).join(" | ")})`);
console.log(failures ? `${failures} check(s) failed` : "all checks passed");
process.exit(failures ? 1 : 0);
