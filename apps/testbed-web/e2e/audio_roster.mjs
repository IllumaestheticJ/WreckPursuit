// Every roster car must be heard (lead round 12b, playtest #47 "some cars are missing sounds").
//
// Usage: node e2e/audio_roster.mjs [roster.json] [--verbose]
//   roster.json: `cargo run -p wp_audio --example roster_trace -- roster.json` (generated into
//   a temp file when omitted). Needs no wasm build: it bundles src/audio/voices.ts with esbuild.
//
// For every car in data/cars as player (6 m), AI racer (25 m), police unit (40 m, siren on) and
// traffic (60 m), the director's command stream (engine sweep idle → 90 % redline, then
// wheelspin + front slide) is rendered through the testbed's real VoicePool into an
// OfflineAudioContext, with each mixer bus on its own channel. Checks, per case and for both
// recorded samples (served by vite exactly like the game) and the procedural fallback:
//   - the Engine bus is audible through the whole sweep (no silent 50 ms window),
//   - recorded mode really plays the recordings (sample plays > 0),
//   - the World bus carries the wheelspin / squeal while the wheels spin,
//   - police units' siren is audible on the Police bus,
// and for one car per engine pack: an engine that starts before its recordings decode stays
// audible, switches to the recordings when they arrive, and leaves no stale voice behind.
import { spawnSync, spawn } from "node:child_process";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const repo = join(root, "../..");
const verbose = process.argv.includes("--verbose");
let tracePath = process.argv.slice(2).find((a) => !a.startsWith("--"));
if (!tracePath) {
  tracePath = join(mkdtempSync(join(tmpdir(), "wp-roster-")), "roster.json");
  const r = spawnSync("cargo", ["run", "-q", "-p", "wp_audio", "--example", "roster_trace", "--", tracePath], {
    cwd: repo,
    stdio: "inherit",
  });
  if (r.status !== 0) throw new Error("roster_trace failed");
}
const trace = JSON.parse(readFileSync(tracePath, "utf8"));

const PORT = 5197;
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
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--autoplay-policy=no-user-gesture-required"],
});
const page = await browser.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
// HTTP errors are reported by URL below (the blank page's favicon.ico 404 is not ours).
page.on("console", (m) => m.type() === "error" && !m.text().startsWith("Failed to load resource") && errors.push(m.text()));
page.on("response", (r) => !r.ok() && !r.url().endsWith("/favicon.ico") && errors.push(`${r.status()} ${r.url()}`));
// Same origin as the game so the packs' relative sample URLs resolve the same way.
await page.goto(URL + "data/audio/audio_config.json");
await page.setContent(`<html><head><base href="${URL}"></head><body></body></html>`);
await page.addScriptTag({ content: bundle.outputFiles[0].text });

// Shared page helpers: render a list of cases back to back, one channel per bus.
await page.evaluate(() => {
  const SR = 24000;
  const BUS_CH = { Engine: 0, World: 1, Police: 2 };
  const GAP = 0.4;
  /** Wait (real time) until no sample is loading. */
  window.waitDecoded = async (pool) => {
    for (let i = 0; i < 600 && pool.sampleStatus().loading > 0; i++) await new Promise((r) => setTimeout(r, 50));
    return pool.sampleStatus();
  };
  window.newRig = (lib, seconds, source) => {
    const ctx = new OfflineAudioContext(3, Math.ceil(seconds * SR), SR);
    ctx.destination.channelInterpretation = "discrete";
    const mixer = new WPA.AudioMixer(ctx);
    const merger = ctx.createChannelMerger(3);
    merger.connect(ctx.destination);
    for (const [b, ch] of Object.entries(BUS_CH)) {
      mixer.buses[b].duck.disconnect();
      mixer.buses[b].duck.connect(merger, 0, ch);
    }
    for (const b of ["Ui", "Music"]) mixer.buses[b].duck.disconnect();
    const pool = new WPA.VoicePool(ctx, mixer, lib);
    pool.source = source;
    return { ctx, mixer, pool };
  };
  /** Apply one frame of commands, entity remapped so cases never share voices. */
  window.applyFrame = (pool, mixer, cmds, entity, active) => {
    for (const c of cmds) {
      if ("Play" in c) {
        const p = c.Play;
        const v = { entity, slot: p.voice.slot };
        pool.play(v, p.bus, p.pack, p.cue, p.looped, p.params, p.synth);
        if (p.looped) active.set(p.voice.slot, p);
      } else if ("Set" in c) {
        pool.set({ entity, slot: c.Set.voice.slot }, c.Set.params);
        const a = active.get(c.Set.voice.slot);
        if (a) a.params = c.Set.params;
      } else if ("Stop" in c) {
        pool.stop({ entity, slot: c.Stop.voice.slot }, c.Stop.fade_s);
        active.delete(c.Stop.voice.slot);
      } else if ("Duck" in c) mixer.duck(c.Duck.bus, c.Duck.gain, c.Duck.fade_s);
    }
    pool.collect();
  };
  /** Per-channel RMS of 50 ms windows over [t0, t1). */
  window.windows = (buf, ch, t0, t1) => {
    const d = buf.getChannelData(ch);
    const w = Math.round(0.05 * SR);
    const out = [];
    for (let s = Math.round(t0 * SR); s + w <= Math.min(Math.round(t1 * SR), d.length); s += w) {
      let sum = 0;
      for (let i = s; i < s + w; i++) sum += d[i] * d[i];
      out.push(Math.sqrt(sum / w));
    }
    return out;
  };
  /** Play counters (an older pool without them reads as zeros instead of throwing). */
  window.played = (pool) => ({ samples: 0, synth: 0, upgraded: 0, ...(pool.played ?? {}) });
  window.ROSTER = { SR, GAP };
});

const SPIN_START = 1.6; // tests/support/roster.rs SWEEP_S
const caseLen = trace.cases[0].frames.length / 60;

const results = {};
for (const source of ["recorded", "procedural"]) {
  const r = await page.evaluate(
    async ({ trace, source, caseLen, SPIN_START }) => {
      const { GAP } = window.ROSTER;
      const n = trace.cases.length;
      const total = n * (caseLen + GAP) + 0.5;
      const { ctx, mixer, pool } = window.newRig(trace.library, total, source);
      const st = await window.waitDecoded(pool);
      const [pos, fwd, up] = trace.listener;
      WPA.setListener(ctx, pos, fwd, up);
      const q = 128 / ctx.sampleRate;
      const perCase = [];
      for (let i = 0; i < n; i++) {
        const c = trace.cases[i];
        const start = i * (caseLen + GAP);
        const entity = 100 + i;
        const active = new Map();
        const rec = { samples0: 0, synth0: 0 };
        perCase.push(rec);
        c.frames.forEach((f, k) => {
          const t = Math.max(q, Math.round((start + f.t) / q) * q);
          ctx.suspend(t).then(() => {
            if (k === 0) {
              rec.samples0 = window.played(pool).samples;
              rec.synth0 = window.played(pool).synth;
            }
            window.applyFrame(pool, mixer, f.commands, entity, active);
            if (k === c.frames.length - 1) {
              for (const s of [...active.keys()]) pool.stop({ entity, slot: s }, 0.05);
              pool.collect();
              rec.samples = window.played(pool).samples - rec.samples0;
              rec.synth = window.played(pool).synth - rec.synth0;
            }
            ctx.resume();
          });
        });
      }
      const buf = await ctx.startRendering();
      const out = trace.cases.map((c, i) => {
        const start = i * (caseLen + GAP);
        const eng = window.windows(buf, 0, start + 0.1, start + SPIN_START);
        const world = window.windows(buf, 1, start + SPIN_START + 0.35, start + caseLen);
        const pol = window.windows(buf, 2, start + 0.2, start + caseLen);
        const db = (x) => 20 * Math.log10(Math.max(x, 1e-9));
        const rms = (a) => Math.sqrt(a.reduce((s, x) => s + x * x, 0) / Math.max(a.length, 1));
        return {
          car: c.car,
          role: c.role,
          pack: c.engine_pack,
          engineMinDb: db(Math.min(...eng)),
          engineDb: db(rms(eng)),
          worldMinDb: db(Math.min(...world)),
          policeMinDb: db(Math.min(...pol)),
          policeDb: db(rms(pol)),
          peakDb: db(Math.max(...eng)),
          samples: perCase[i].samples,
          synth: perCase[i].synth,
        };
      });
      return { status: st, cases: out };
    },
    { trace, source, caseLen, SPIN_START },
  );
  results[source] = r;
  if (source === "recorded") {
    check(r.status.loaded === r.status.total && r.status.failed.length === 0, `recorded samples decoded (${r.status.loaded}/${r.status.total})`);
  }
  // Audible: the quietest 50 ms window above -70 dBFS (traffic at 60 m sits around -40 dBFS).
  const FLOOR = -70;
  const silentEngine = r.cases.filter((c) => !(c.engineMinDb > FLOOR));
  check(
    silentEngine.length === 0,
    `${source}: every car's engine audible in every role (${r.cases.length} cases)` +
      (silentEngine.length ? `; silent: ${silentEngine.map((c) => `${c.car}/${c.role} ${c.engineMinDb.toFixed(0)} dB`).join(", ")}` : ""),
  );
  const noSpin = r.cases.filter((c) => !(c.worldMinDb > FLOOR));
  check(
    noSpin.length === 0,
    `${source}: wheelspin / squeal audible for every case` + (noSpin.length ? `; silent: ${noSpin.map((c) => `${c.car}/${c.role}`).join(", ")}` : ""),
  );
  const police = r.cases.filter((c) => c.role === "police");
  const noSiren = police.filter((c) => !(c.policeMinDb > FLOOR));
  check(
    noSiren.length === 0,
    `${source}: siren audible on every car as police (${police.length})` + (noSiren.length ? `; silent: ${noSiren.map((c) => c.car).join(", ")}` : ""),
  );
  if (source === "recorded") {
    const noRec = r.cases.filter((c) => c.samples === 0);
    check(noRec.length === 0, `recorded: every case plays recordings` + (noRec.length ? `; none: ${noRec.map((c) => `${c.car}/${c.role}`).join(", ")}` : ""));
  } else {
    const rec = r.cases.filter((c) => c.samples > 0);
    check(rec.length === 0, `procedural: no recordings played` + (rec.length ? ` (${rec.length} cases did)` : ""));
  }
}

// Recorded vs procedural level per car (player role): a mode much quieter reads as "silent".
const rows = [];
for (let i = 0; i < trace.cases.length; i++) {
  const a = results.recorded.cases[i];
  const b = results.procedural.cases[i];
  rows.push({ ...a, procDb: b.engineDb, diff: a.engineDb - b.engineDb, procSirenDb: b.policeDb });
}
const player = rows.filter((r) => r.role === "player");
// Before round 12b the recorded inline-4s sat 17 dB under their fallback (owner: "silent").
const lopsided = player.filter((r) => Math.abs(r.diff) > 6);
const sirens = rows.filter((r) => r.role === "police");
const sirenOff = sirens.filter((r) => Math.abs(r.policeDb - r.procSirenDb) > 6);
check(
  sirenOff.length === 0,
  `recorded and procedural sirens within 6 dB` + (sirenOff.length ? `; ${sirenOff.map((r) => `${r.car} ${(r.policeDb - r.procSirenDb).toFixed(1)} dB`).join(", ")}` : ""),
);
check(
  lopsided.length === 0,
  `recorded and procedural engines within 6 dB for every car` +
    (lopsided.length ? `; ${lopsided.map((r) => `${r.car} ${r.diff.toFixed(1)} dB`).join(", ")}` : ""),
);
if (verbose) {
  console.log("car                  role     pack              rec dB (min)    proc dB  world min  siren rec/proc dB");
  for (const r of rows) {
    console.log(
      `${r.car.padEnd(20)} ${r.role.padEnd(8)} ${r.pack.padEnd(17)} ${r.engineDb.toFixed(1).padStart(6)} (${r.engineMinDb.toFixed(1).padStart(6)}) ${r.procDb
        .toFixed(1)
        .padStart(8)} ${r.worldMinDb.toFixed(1).padStart(9)} ${r.role === "police" ? `${r.policeDb.toFixed(1)} / ${r.procSirenDb.toFixed(1)}` : ""}`,
    );
  }
}

// Recordings arriving mid-drive: one car per engine pack, player role. Fetches are held until
// 1.0 s into the render, then the render waits (suspended) for every file to decode.
const perPack = new Map();
trace.cases.forEach((c, i) => c.role === "player" && !perPack.has(c.engine_pack) && perPack.set(c.engine_pack, i));
await page.evaluate(() => {
  const realFetch = window.fetch.bind(window);
  window.gate = { open: false, waiters: [] };
  window.fetch = (...a) =>
    window.gate.open ? realFetch(...a) : new Promise((r) => window.gate.waiters.push(() => r(realFetch(...a))));
  window.openGate = () => {
    window.gate.open = true;
    for (const w of window.gate.waiters.splice(0)) w();
  };
});
for (const [pack, idx] of perPack) {
  const r = await page.evaluate(
    async ({ trace, idx, caseLen }) => {
      window.gate.open = false;
      const c = trace.cases[idx];
      const { ctx, mixer, pool } = window.newRig(trace.library, caseLen + 0.3, "recorded");
      const [pos, fwd, up] = trace.listener;
      WPA.setListener(ctx, pos, fwd, up);
      const q = 128 / ctx.sampleRate;
      const active = new Map();
      let before = null;
      let after = null;
      ctx.suspend(1.0).then(async () => {
        before = window.played(pool);
        window.openGate();
        await window.waitDecoded(pool);
        ctx.resume();
      });
      c.frames.forEach((f, k) => {
        const t = Math.max(q, Math.round(f.t / q) * q);
        if (Math.abs(t - 1.0) < q / 2) return; // the gate suspend owns this quantum
        ctx.suspend(t).then(() => {
          window.applyFrame(pool, mixer, f.commands, 1, active);
          if (k === c.frames.length - 1) after = { ...window.played(pool), size: pool.size, active: active.size };
          ctx.resume();
        });
      });
      const buf = await ctx.startRendering();
      const eng = window.windows(buf, 0, 0.1, 1.6);
      return { before, after, minDb: 20 * Math.log10(Math.max(Math.min(...eng), 1e-9)) };
    },
    { trace, idx, caseLen },
  );
  const ok =
    r.before.samples === 0 && r.after.upgraded > 0 && r.after.samples > 0 && r.minDb > -70 && r.after.size === r.after.active;
  check(
    ok,
    `${pack}: engine started before decode stays audible (min ${r.minDb.toFixed(1)} dB), switches to recordings (${r.after.upgraded} upgraded), ` +
      `no stale voices (${r.after.size} playing / ${r.after.active} active)`,
  );
}

check(errors.length === 0, `no page errors${errors.length ? ": " + errors.slice(0, 5).join(" | ") : ""}`);
await browser.close();
server.kill("SIGTERM");
console.log(failures ? `${failures} FAILED` : "all audio roster checks passed");
process.exit(failures ? 1 : 0);
