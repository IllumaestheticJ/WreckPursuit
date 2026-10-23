// Impact and horn synthesis (owner row 74): offline renders + checks, and a live latency check.
//
// Usage: node e2e/audio_impacts.mjs [impact_trace.json] [--no-live] [--no-wav]
//   impact_trace.json: `cargo run --release -p wp_audio --example impact_trace -- out.json`
//   (generated into a temp file when omitted). Real sim crashes (simcli scenarios, Rapier +
//   wp_damage) fed to the director at 60 Hz; glass and horns are short director scripts.
//
// 1. Offline: each case's command stream is played through the testbed's real VoicePool /
//    AudioMixer (esbuild bundle of src/audio/voices.ts) in an OfflineAudioContext, with
//    recorded samples served by vite like the game. The stereo master is written to
//    docs/media/audio/impacts/<case>.wav (24 kHz, 16 bit); the Engine / World / Police buses
//    are analysed per channel (glass / horn scripts: Engine bus muted). Checks:
//      - an impact is audible ≤ 30 ms after the contact event (sim time → rendered audio),
//      - 20 < 50 < 90 km/h: louder and more bass; the synthesis adds punch and bass over the
//        recordings alone; a heavy hit ducks the engine bus (sidechain); the crunch follows
//        the deformation (longer, denser for harder hits); interior = more bass, less treble,
//      - glass: crack onset + a tinkling tail (> 4 kHz) lasting ≥ 0.5 s, tempered > laminated,
//      - wall scrape: continuous while sliding, sparks (> 5 kHz) present,
//      - horns held 2.0 s: sound 2.0 ± 0.05 s, onset ≤ 30 ms, steady (no 20 ms dip > 6 dB),
//        −40 dB ≤ 60 ms after release; the damaged horn is weaker and rattles; air horns are
//        lower / different from the disc pair; the recorded fallback still holds,
//      - no NaN, no clipping.
// 2. Live (needs the wasm build, like e2e/audio.mjs): the testbed runs the barrier crash
//    scenario; the impact voice must be built ≤ 30 ms after the frame that drained the
//    contact event and start on the audio clock without delay.
import { spawnSync, spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";
import { chromium } from "playwright-core";
import { chromiumPath } from "./chromium.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const repo = join(root, "../..");
const args = process.argv.slice(2);
const live = !args.includes("--no-live");
const wav = !args.includes("--no-wav");
let tracePath = args.find((a) => !a.startsWith("--"));
if (!tracePath) {
  tracePath = join(mkdtempSync(join(tmpdir(), "wp-impacts-")), "impact_trace.json");
  const r = spawnSync("cargo", ["run", "-q", "--release", "-p", "wp_audio", "--example", "impact_trace", "--", tracePath], {
    cwd: repo,
    stdio: "inherit",
  });
  if (r.status !== 0) throw new Error("impact_trace failed");
}
const trace = JSON.parse(readFileSync(tracePath, "utf8"));
const outDir = join(repo, "docs/media/audio/impacts");

const PORT = 5196;
const URL = `http://localhost:${PORT}/`;
const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
async function waitForServer() {
  for (let i = 0; i < 150; i++) {
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
  entryPoints: [join(root, "src/audio/voices.ts")],
  bundle: true,
  write: false,
  format: "iife",
  globalName: "WPA",
  target: "es2022",
});

await waitForServer();
const browser = await chromium.launch({
  executablePath: chromiumPath(),
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
page.on("response", (r) => !r.ok() && !r.url().endsWith("/favicon.ico") && errors.push(`${r.status()} ${r.url()}`));
await page.goto(URL + "data/audio/audio_config.json");
await page.setContent(`<html><head><base href="${URL}"></head><body></body></html>`);
await page.addScriptTag({ content: bundle.outputFiles[0].text });

await page.evaluate(() => {
  const SR = 24000;
  // Channels: 0-1 master (stereo, after the limiter), 2 Engine, 3 World, 4 Police.
  window.renderCase = async (lib, c, opts) => {
    // Same recordings / scatter on every run: the pool picks files with Math.random.
    let seed = 0x5eed;
    Math.random = () => {
      seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
      return seed / 4294967296;
    };
    const len = c.frames[c.frames.length - 1].t + (opts.tail ?? 0.8);
    const ctx = new OfflineAudioContext(5, Math.ceil(len * SR), SR);
    ctx.destination.channelInterpretation = "discrete";
    const mixer = new WPA.AudioMixer(ctx);
    const merger = ctx.createChannelMerger(5);
    merger.connect(ctx.destination);
    // Master: the mixer's real output stage (compressor + soft clip) into channels 0-1.
    mixer.out.disconnect();
    const split = ctx.createChannelSplitter(2);
    mixer.out.connect(split);
    split.connect(merger, 0, 0);
    split.connect(merger, 1, 1);
    for (const [b, ch] of Object.entries({ Engine: 2, World: 3, Police: 4 })) mixer.buses[b].duck.connect(merger, 0, ch);
    // Director-only scripts (glass, horns) have a car idling 6 m away: its engine is muted so
    // the subject is what you hear (crashes are the full mix).
    if (c.extra?.car) mixer.buses.Engine.user.gain.value = 0;
    const pool = new WPA.VoicePool(ctx, mixer, lib);
    pool.source = opts.source ?? "recorded";
    pool.impacts.enabled = opts.synth !== false;
    for (let i = 0; i < 600 && pool.sampleStatus().loading > 0; i++) await new Promise((r) => setTimeout(r, 50));
    const q = 128 / SR;
    const apply = (f) => {
      const [pos, fwd, up] = f.listener;
      WPA.setListener(ctx, pos, fwd, up);
      for (const cmd of f.commands) {
        if ("Play" in cmd) {
          const p = cmd.Play;
          pool.play(p.voice, p.bus, p.pack, p.cue, p.looped, p.params, p.synth);
        } else if ("Set" in cmd) pool.set(cmd.Set.voice, cmd.Set.params);
        else if ("Stop" in cmd) pool.stop(cmd.Stop.voice, cmd.Stop.fade_s);
        else if ("Duck" in cmd) mixer.duck(cmd.Duck.bus, cmd.Duck.gain, cmd.Duck.fade_s);
      }
      pool.collect();
    };
    // Frame k is applied at its sim time (the director runs once per 4 ticks, like at 60 fps).
    c.frames.forEach((f, k) => {
      const t = Math.round(f.t / q) * q;
      if (k === 0 || t <= 0) {
        apply(f);
        return;
      }
      ctx.suspend(t).then(() => {
        apply(f);
        ctx.resume();
      });
    });
    const t0 = performance.now();
    const buf = await ctx.startRendering();
    const renderMs = performance.now() - t0;
    return { buf, pool, renderMs, len };
  };
  /** RMS envelope (dBFS) of `data` in windows of `w` s. */
  window.env = (d, w) => {
    const n = Math.round(w * SR);
    const out = [];
    for (let s = 0; s + n <= d.length; s += n) {
      let sum = 0;
      for (let i = s; i < s + n; i++) sum += d[i] * d[i];
      out.push(10 * Math.log10(Math.max(sum / n, 1e-18)));
    }
    return out;
  };
  /** One-pole low / high split (cheap band checks). */
  window.lowpass = (d, f) => {
    const a = Math.exp((-2 * Math.PI * f) / SR);
    const o = new Float32Array(d.length);
    let y = 0;
    for (let i = 0; i < d.length; i++) o[i] = y = (1 - a) * d[i] + a * y;
    return o;
  };
  window.highpass = (d, f) => {
    const lp = window.lowpass(d, f);
    const o = new Float32Array(d.length);
    for (let i = 0; i < d.length; i++) o[i] = d[i] - lp[i];
    return o;
  };
  /** dB of the RMS of `d` over [t0, t1). */
  window.rmsDb = (d, t0, t1) => {
    const a = Math.max(0, Math.round(t0 * SR));
    const b = Math.min(d.length, Math.round(t1 * SR));
    let s = 0;
    for (let i = a; i < b; i++) s += d[i] * d[i];
    return 10 * Math.log10(Math.max(s / Math.max(1, b - a), 1e-18));
  };
  window.peakDb = (d, t0, t1) => {
    let m = 0;
    for (let i = Math.max(0, Math.round(t0 * SR)); i < Math.min(d.length, Math.round(t1 * SR)); i++) m = Math.max(m, Math.abs(d[i]));
    return 20 * Math.log10(Math.max(m, 1e-9));
  };
  /** 16-bit stereo WAV of channels 0-1 as base64. */
  window.wavB64 = (buf) => {
    const L = buf.getChannelData(0);
    const R = buf.getChannelData(1);
    const n = L.length;
    const bytes = new Uint8Array(44 + n * 4);
    const dv = new DataView(bytes.buffer);
    const str = (o, s) => [...s].forEach((ch, i) => dv.setUint8(o + i, ch.charCodeAt(0)));
    str(0, "RIFF");
    dv.setUint32(4, 36 + n * 4, true);
    str(8, "WAVE");
    str(12, "fmt ");
    dv.setUint32(16, 16, true);
    dv.setUint16(20, 1, true);
    dv.setUint16(22, 2, true);
    dv.setUint32(24, buf.sampleRate, true);
    dv.setUint32(28, buf.sampleRate * 4, true);
    dv.setUint16(32, 4, true);
    dv.setUint16(34, 16, true);
    str(36, "data");
    dv.setUint32(40, n * 4, true);
    for (let i = 0; i < n; i++) {
      dv.setInt16(44 + i * 4, Math.max(-1, Math.min(1, L[i])) * 32767, true);
      dv.setInt16(46 + i * 4, Math.max(-1, Math.min(1, R[i])) * 32767, true);
    }
    let s = "";
    for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return btoa(s);
  };
});

// Analyse one case in the page; returns numbers only (plus the WAV when asked).
async function analyse(name, opts = {}) {
  return page.evaluate(
    async ({ lib, c, opts }) => {
      const { buf, pool, renderMs, len } = await window.renderCase(lib, c, opts);
      const ch = (i) => buf.getChannelData(i);
      const mix = new Float32Array(ch(0).length);
      for (let i = 0; i < mix.length; i++) mix[i] = 0.5 * (ch(0)[i] + ch(1)[i]);
      let nan = false;
      let peak = 0;
      for (const i of [0, 1]) for (const x of ch(i)) {
        if (!Number.isFinite(x)) nan = true;
        peak = Math.max(peak, Math.abs(x));
      }
      const world = ch(3);
      const engine = ch(2);
      const builds = pool.impacts.log.map((e) => e.buildMs);
      const r = { renderMs, len, nan, peakDb: 20 * Math.log10(Math.max(peak, 1e-9)), built: { ...pool.impacts.built }, played: { ...pool.played }, builds };
      const tc = c.extra?.first_contact_s;
      if (tc != null) {
        // Onset: first 2 ms window after the contact halfway (in dB) from the World bus level
        // just before it to the hit's peak (at least 6 dB over the background).
        const w = 0.002;
        const e = window.env(world, w);
        const i0 = Math.floor(tc / w);
        const pre = Math.max(-90, ...e.slice(Math.max(0, i0 - 50), i0 - 1));
        const top = Math.max(...e.slice(i0, i0 + 75));
        const thr = pre + Math.min(20, (top - pre) / 2);
        const on = top - pre >= 6 ? e.findIndex((x, i) => i >= i0 - 1 && x > thr) : -1;
        r.onsetMs = on < 0 ? null : (on * w - tc) * 1000;
        r.onsetRiseDb = top - pre;
        r.hitPeakDb = window.peakDb(world, tc, tc + 0.15);
        r.hitRmsDb = window.rmsDb(world, tc, tc + 0.2);
        const low = window.lowpass(window.lowpass(world, 120), 120);
        r.bassDb = window.rmsDb(low, tc, tc + 0.15);
        const high = window.highpass(world, 3000);
        r.trebleDb = window.rmsDb(high, tc, tc + 0.15);
        // Crunch / debris body: 100–400 ms after the hit.
        r.tailDb = window.rmsDb(world, tc + 0.1, tc + 0.4);
        // Engine bus around the hit (sidechain).
        r.enginePreDb = window.rmsDb(engine, tc - 0.15, tc - 0.01);
        r.engineHitDb = window.rmsDb(engine, tc + 0.01, tc + 0.08);
      }
      if (c.glass?.length) {
        const hf = window.highpass(window.highpass(world, 4000), 4000);
        const e = window.env(hf, 0.01);
        r.glass = c.glass.map((tg, k) => {
          const end = c.glass[k + 1] ?? len;
          const i0 = Math.round(tg / 0.01);
          const peakE = Math.max(...e.slice(i0, i0 + 10));
          let last = i0;
          for (let i = i0; i < Math.round(end / 0.01); i++) if (e[i] > peakE - 40) last = i;
          const onset = e.findIndex((x, i) => i >= i0 - 1 && x > peakE - 12);
          return { onsetMs: (onset * 0.01 - tg) * 1000, tailS: (last - i0) * 0.01, peakDb: peakE };
        });
      }
      if (c.extra?.hold_s) {
        const w = 0.005;
        const e = window.env(world, w);
        const press = c.extra.press_s;
        const hold = c.extra.hold_s;
        const held = e.slice(Math.round((press + 0.05) / w), Math.round((press + hold) / w));
        const med = [...held].sort((a, b) => a - b)[Math.floor(held.length / 2)];
        let first = -1;
        let last = -1;
        e.forEach((x, i) => {
          if (x > med - 20) {
            if (first < 0) first = i;
            last = i;
          }
        });
        let worst20 = Infinity;
        const w20 = window.env(world, 0.02);
        for (let i = Math.round((press + 0.05) / 0.02); i < Math.round((press + hold) / 0.02) - 1; i++) worst20 = Math.min(worst20, w20[i]);
        const quiet = e.findIndex((x, i) => i * w >= press + hold && e.slice(i).every((y) => y < med - 40));
        // Envelope roughness: std-dev of 10 ms levels while held (rattle).
        const e10 = window.env(world, 0.01).slice(Math.round((press + 0.1) / 0.01), Math.round((press + hold) / 0.01));
        const mean = e10.reduce((a, b) => a + b, 0) / e10.length;
        const sd = Math.sqrt(e10.reduce((a, b) => a + (b - mean) ** 2, 0) / e10.length);
        // Spectral centroid via 4 bands.
        const bands = [200, 600, 1500, 4000].map((f) => window.rmsDb(window.lowpass(world, f), press + 0.2, press + hold));
        r.horn = {
          med,
          onsetMs: (first * w - press) * 1000,
          soundS: (last + 1 - first) * w,
          worst20: worst20 - med,
          releaseMs: (quiet * w - press - hold) * 1000,
          roughDb: sd,
          bands,
        };
      }
      if (c.name.includes("scrape")) {
        const sets = [];
        for (const f of c.frames) for (const cmd of f.commands) {
          const v = cmd.Play?.voice ?? cmd.Set?.voice;
          if (v && (v.slot & 0x40000000) && !(v.slot & 0x80000000)) sets.push(f.t);
        }
        const t0 = Math.min(...sets);
        const t1 = Math.max(...sets);
        const e = window.env(world, 0.05).slice(Math.round((t0 + 0.1) / 0.05), Math.round((t1 - 0.1) / 0.05));
        const med = [...e].sort((a, b) => a - b)[Math.floor(e.length / 2)];
        r.scrape = {
          t0,
          t1,
          med,
          worst: Math.min(...e) - med,
          sparksDb: window.rmsDb(window.highpass(window.highpass(world, 5000), 5000), t0 + 0.1, t1 - 0.1),
          bodyDb: window.rmsDb(world, t0 + 0.1, t1 - 0.1),
        };
      }
      if (opts.wav) r.wav = window.wavB64(buf);
      return r;
    },
    { lib: trace.library, c: trace.cases.find((x) => x.name === name), opts },
  );
}

const fmt = (x) => (x == null ? "—" : x.toFixed(1));
const res = {};
if (wav) mkdirSync(outDir, { recursive: true });
let renderMs = 0;
let audioS = 0;
for (const c of trace.cases) {
  const r = await analyse(c.name, { wav });
  if (r.wav) {
    writeFileSync(join(outDir, `${c.name}.wav`), Buffer.from(r.wav, "base64"));
    delete r.wav;
  }
  res[c.name] = r;
  renderMs += r.renderMs;
  audioS += r.len;
  check(!r.nan && r.peakDb <= 0.0, `${c.name}: no NaN, master peak ${fmt(r.peakDb)} dBFS`);
}
const builds = Object.values(res).flatMap((r) => r.builds);
console.log(`      impact voice build (main thread): mean ${(builds.reduce((a, b) => a + b, 0) / builds.length).toFixed(3)} ms, max ${Math.max(...builds).toFixed(3)} ms over ${builds.length} voices`);
console.log(`      offline render: ${(renderMs / 1000).toFixed(2)} s for ${audioS.toFixed(1)} s of audio (${((renderMs / 1000 / audioS) * 100).toFixed(0)} % of real time, all buses)`);

// Recordings only (the fallback setting) for comparison.
const rec = {};
for (const n of ["frontal_20kmh", "frontal_50kmh", "frontal_90kmh", "wall_scrape", "horn_disc_pair", "horn_air_truck", "horn_air_police"]) rec[n] = await analyse(n, { synth: false });
const proc = {};
for (const n of ["frontal_90kmh", "horn_disc_pair", "horn_air_truck"]) proc[n] = await analyse(n, { source: "procedural" });

console.log("\n      case                     onset  hit pk  hit rms  bass  treble  tail   eng dip   (synth | recordings only)");
for (const n of ["frontal_20kmh", "frontal_50kmh", "frontal_90kmh", "frontal_50kmh_interior", "sideswipe_scrape", "wall_scrape"]) {
  const r = res[n];
  const q = rec[n];
  const row = (x) =>
    `${fmt(x.onsetMs).padStart(5)} ${fmt(x.hitPeakDb).padStart(7)} ${fmt(x.hitRmsDb).padStart(8)} ${fmt(x.bassDb).padStart(5)} ${fmt(x.trebleDb).padStart(7)} ${fmt(x.tailDb).padStart(5)} ${fmt(x.engineHitDb - x.enginePreDb).padStart(8)}`;
  console.log(`      ${n.padEnd(24)} ${row(r)}${q ? `   | ${row(q)}` : ""}`);
}

// --- impacts ---------------------------------------------------------------------------
// (The car-to-car sideswipe's first touch is too soft to stand out from two passing cars.)
for (const n of ["frontal_20kmh", "frontal_50kmh", "frontal_90kmh", "frontal_50kmh_interior", "wall_scrape"]) {
  const r = res[n];
  check(r.onsetMs != null && r.onsetMs >= -1 && r.onsetMs <= 30, `${n}: impact audible ${fmt(r.onsetMs)} ms after the contact event (≤ 30 ms)`);
}
const [f20, f50, f90] = ["frontal_20kmh", "frontal_50kmh", "frontal_90kmh"].map((n) => res[n]);
check(f20.hitRmsDb + 2 < f50.hitRmsDb && f50.hitRmsDb + 1 < f90.hitRmsDb, `louder with speed: 20 / 50 / 90 km/h hit RMS ${fmt(f20.hitRmsDb)} / ${fmt(f50.hitRmsDb)} / ${fmt(f90.hitRmsDb)} dB`);
check(f20.bassDb + 3 < f50.bassDb && f50.bassDb < f90.bassDb, `heavier with speed: bass (< 120 Hz) ${fmt(f20.bassDb)} / ${fmt(f50.bassDb)} / ${fmt(f90.bassDb)} dB`);
check(f90.tailDb > f20.tailDb + 3, `crunch / debris after the hit follows the deformation: 20 km/h ${fmt(f20.tailDb)} dB, 90 km/h ${fmt(f90.tailDb)} dB`);
for (const n of ["frontal_50kmh", "frontal_90kmh"]) {
  const r = res[n];
  const q = rec[n];
  check(r.hitPeakDb >= q.hitPeakDb + 2 && r.bassDb >= q.bassDb + 4, `${n}: synthesis hits harder than the recordings alone (peak +${fmt(r.hitPeakDb - q.hitPeakDb)} dB, bass +${fmt(r.bassDb - q.bassDb)} dB)`);
}
const dipS = f90.engineHitDb - f90.enginePreDb;
const dipR = rec.frontal_90kmh.engineHitDb - rec.frontal_90kmh.enginePreDb;
check(dipS - dipR <= -3, `90 km/h: sidechain ducks the engine bus during the hit (${fmt(dipS)} dB vs ${fmt(dipR)} dB without synthesis)`);
check(f20.engineHitDb - f20.enginePreDb > dipS + 2, `20 km/h ducks less than 90 km/h (${fmt(f20.engineHitDb - f20.enginePreDb)} vs ${fmt(dipS)} dB)`);
const fin = res.frontal_50kmh_interior;
check(fin.bassDb - fin.trebleDb > f50.bassDb - f50.trebleDb + 3, `interior: more body, less air than exterior (bass − treble ${fmt(fin.bassDb - fin.trebleDb)} vs ${fmt(f50.bassDb - f50.trebleDb)} dB)`);
check((res.frontal_90kmh.built.crunch ?? 0) > 0 && (res.frontal_90kmh.built.impact_body ?? 0) > 0, `crunch and body voices built (${JSON.stringify(res.frontal_90kmh.built)})`);
check((proc.frontal_90kmh.built.crunch ?? 0) > 0 && proc.frontal_90kmh.hitPeakDb > f90.hitPeakDb - 6, `procedural mode: synthesis still plays (noise-grain crunch), hit peak ${fmt(proc.frontal_90kmh.hitPeakDb)} dB`);
check(!(rec.frontal_90kmh.built.impact_body > 0) && !(rec.frontal_90kmh.built.crunch > 0), "recordings-only setting: no synthesised impact voices");

// --- glass ---------------------------------------------------------------------------------
const g = res.glass_break.glass;
console.log(`      glass: ${g.map((x) => `onset ${fmt(x.onsetMs)} ms, tail ${x.tailS.toFixed(2)} s, ${fmt(x.peakDb)} dB`).join(" | ")}`);
check(g.length === 2 && g.every((x) => x.onsetMs <= 30), "glass: shatter onset within 30 ms of each GlassShattered event");
check(g.every((x) => x.tailS >= 0.5), "glass: tinkling debris tail (> 4 kHz) lasts ≥ 0.5 s");
check(g[0].tailS > g[1].tailS, `glass: tempered rear window sheds a longer tail than the laminated windshield (${g[0].tailS.toFixed(2)} vs ${g[1].tailS.toFixed(2)} s)`);
check(res.frontal_90kmh.glass?.[0]?.tailS >= 0.3, `90 km/h crash broke the windshield and its tail is heard (${res.frontal_90kmh.glass?.[0]?.tailS?.toFixed(2)} s)`);

// --- scrape --------------------------------------------------------------------------------
const ws = res.wall_scrape.scrape;
console.log(`      wall scrape ${ws.t0.toFixed(2)}–${ws.t1.toFixed(2)} s: median ${fmt(ws.med)} dB, worst 50 ms ${fmt(ws.worst)} dB, sparks ${fmt(ws.sparksDb)} dB`);
check(ws.t1 - ws.t0 > 1.5 && ws.worst > -20, "wall scrape: continuous while sliding (no 50 ms window 20 dB under the median)");
// The friction synth adds the spark / grit band over the recorded scrape loop (an absolute level
// vs the body depended on how far the car slides from the ear; re-based after round 13).
const wsr = rec.wall_scrape.scrape;
check(ws.sparksDb >= wsr.sparksDb + 3, `wall scrape: synthesis adds sparks / grit above 5 kHz (${fmt(ws.sparksDb)} dB vs ${fmt(wsr.sparksDb)} dB recordings only; body ${fmt(ws.bodyDb)} dB)`);
check((res.wall_scrape.built.friction ?? 0) > 0, "friction synthesis built for the scrape");

// --- horns ---------------------------------------------------------------------------------
const hornRow = (label, h) =>
  `${label}: sounds ${h.soundS.toFixed(3)} s at ${fmt(h.med)} dB, onset ${fmt(h.onsetMs)} ms, worst 20 ms ${fmt(h.worst20)} dB, quiet ${fmt(h.releaseMs)} ms after release, rough ${h.roughDb.toFixed(2)} dB`;
for (const n of ["horn_disc_pair", "horn_disc_pair_damaged", "horn_air_truck", "horn_air_police"]) {
  const h = res[n].horn;
  // The damaged horn chatters by design (≤ 12 dB); the others must be steady (≤ 6 dB).
  const steady = n.endsWith("damaged") ? h.worst20 > -12 : h.worst20 > -6;
  check(Math.abs(h.soundS - 2.0) <= 0.05 && h.onsetMs <= 30 && h.releaseMs <= 60 && steady, hornRow(n, h));
}
for (const n of ["horn_disc_pair", "horn_air_truck", "horn_air_police"]) {
  const h = rec[n].horn;
  check(Math.abs(h.soundS - 2.0) <= 0.1 && h.onsetMs <= 30 && h.releaseMs <= 60, hornRow(`${n} (recorded fallback)`, h));
}
for (const n of ["horn_disc_pair", "horn_air_truck"]) {
  const d = res[n].horn.med - proc[n].horn.med;
  check(Math.abs(d) <= 1, `${n}: same model in recorded and procedural mode (${fmt(d)} dB)`);
  const r = res[n].horn.med - rec[n].horn.med;
  check(Math.abs(r) <= 6, `${n}: modelled horn within 6 dB of the recorded fallback (${fmt(r)} dB)`);
}
const hd = res.horn_disc_pair.horn;
const hdd = res.horn_disc_pair_damaged.horn;
check(hdd.med < hd.med - 3 && hdd.roughDb > hd.roughDb + 1, `damaged front end: horn weaker (${fmt(hdd.med - hd.med)} dB) and rattly (envelope roughness ${hdd.roughDb.toFixed(2)} vs ${hd.roughDb.toFixed(2)} dB)`);
const tilt = (h) => h.bands[1] - h.bands[3]; // < 600 Hz vs < 4 kHz: more low end = larger
check(tilt(res.horn_air_truck.horn) > tilt(hd) + 1, `truck air horn is lower than the car horn (low-band tilt ${fmt(tilt(res.horn_air_truck.horn))} vs ${fmt(tilt(hd))} dB)`);

// --- live testbed: contact event → impact voice --------------------------------------------
if (live) {
  const lp = await (await browser.newContext({ viewport: { width: 1200, height: 760 } })).newPage();
  lp.on("pageerror", (e) => errors.push(String(e)));
  await lp.goto(URL);
  await lp.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
  await lp.keyboard.press("KeyX");
  await lp.waitForFunction(() => window.__wpAudio?.running(), null, { timeout: 10000 });
  await lp.waitForTimeout(500);
  await lp.evaluate(() => window.__wp.runScenario("crash/barrier_frontal.ron", 50));
  await lp.waitForFunction(() => window.__wp.sim.scenarioResult()?.done === true, null, { timeout: 120000 });
  const r = await lp.evaluate(() => {
    const a = window.__wpAudio;
    const log = a.impactLog().filter((e) => e.name === "impact_body");
    const first = log[0];
    // The frame that drained the first contact event: the latest contact frame at or before
    // the voice (both stamped with performance.now()).
    const frames = a.contactFrames.filter((t) => first && t <= first.wall + 0.001);
    const drained = frames.length ? frames[frames.length - 1] : null;
    return {
      voices: log.length,
      latencyMs: first && drained != null ? first.wall - drained : null,
      scheduleMs: first ? (first.audio - first.now) * 1000 : null,
      outputMs: (() => {
        const ctx = a.ctx;
        return ctx ? ((ctx.baseLatency ?? 0) + (ctx.outputLatency ?? 0)) * 1000 : null;
      })(),
    };
  });
  console.log(`      live: ${r.voices} impact voices, frame→voice ${fmt(r.latencyMs)} ms, scheduled +${fmt(r.scheduleMs)} ms, device latency ${fmt(r.outputMs)} ms`);
  check(r.voices > 0 && r.latencyMs != null && r.latencyMs + Math.max(0, r.scheduleMs) <= 30, `live testbed: impact voice ≤ 30 ms after the contact event reached the audio system (${fmt(r.latencyMs)} ms + ${fmt(r.scheduleMs)} ms)`);
}

check(errors.length === 0, `no page errors${errors.length ? `: ${errors.slice(0, 5).join(" | ")}` : ""}`);
await browser.close();
server.kill("SIGTERM");
console.log(failures ? `\n${failures} check(s) failed` : "\nall impact / horn checks passed");
process.exit(failures ? 1 : 0);
