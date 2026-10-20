// Browser frame profile of the city chase (browser perf pass, docs/integration/browser-perf.md).
// Usage: node e2e/frame_profile.mjs [chase|rain] [seconds]   (needs `npm run wasm`; own Vite)
// Env: VIEWPORT=WxH (default 1280x720), CITY=<id> (default the app's default, vesper_v2),
// QUERY=<url params> (e.g. gfx=high), EVAL=<js> run before measuring, STACKS=<fn> allocation paths,
// JSON=1 prints only the JSON line.
//
// Per frame it reports: the frame-loop sections (src/perf.ts), the wall frame time and what is
// left of it (SwiftShader raster / compositor wait), one GPU-synced render (render + 1-pixel
// readPixels), draw calls, triangles, wasm↔JS calls (everything through `sim` and the audio
// bridge), and allocated bytes (V8 sampling heap profiler incl. objects already collected =
// GC pressure) with the top allocating functions. Audio runs (autoplay allowed, recorded packs).
// Headless SwiftShader numbers are only comparable with each other, not with a real GPU.
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = Number(process.env.PORT ?? 5194);
// QUERY=gfx=high etc.: extra URL parameters (e.g. force a graphics quality profile).
const qs = [process.env.CITY ? `city=${process.env.CITY}` : "", process.env.QUERY ?? ""].filter(Boolean).join("&");
const URL = `http://localhost:${PORT}/${qs ? `?${qs}` : ""}`;
const [which = "chase", secs = "12"] = process.argv.slice(2);
const jsonOnly = process.env.JSON === "1";

// Own process group, so the exit handler also stops the vite process npx starts.
const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe", detached: true });
process.on("exit", () => {
  try {
    process.kill(-server.pid, "SIGTERM");
  } catch {}
});
for (let i = 0; ; i++) {
  try {
    if ((await fetch(`http://localhost:${PORT}/`)).ok) break;
  } catch {}
  if (i > 150) throw new Error("vite did not start");
  await new Promise((r) => setTimeout(r, 200));
}
const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: [
    "--use-gl=angle",
    "--use-angle=swiftshader",
    "--enable-unsafe-swiftshader",
    "--ignore-gpu-blocklist",
    "--autoplay-policy=no-user-gesture-required",
  ],
});
const [vw, vh] = (process.env.VIEWPORT ?? "1280x720").split("x").map(Number);
const page = await (await browser.newContext({ viewport: { width: vw, height: vh } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 90000 });
await page.evaluate((rain) => window.__wp.startCityChase(undefined, "Arcade", rain), which === "rain");
// The key press also starts the AudioContext (recorded packs load in the warm-up).
await page.keyboard.down("KeyW");
await page.waitForTimeout(5000);
// EVAL=<js>: run in the page before measuring (A/B experiments, e.g. hide a layer).
if (process.env.EVAL) await page.evaluate(process.env.EVAL);

// Count wasm↔JS calls: wrap every function of the `sim` wrapper and the audio bridge (the only
// two modules that call the wasm exports; same module instances as the app's).
await page.evaluate(async () => {
  const { audioBridge } = await import("/src/audio/bridge.ts");
  const counts = (window.__wasmCalls = {});
  for (const [name, obj] of [
    ["sim", window.__wp.sim],
    ["audio", audioBridge],
  ]) {
    for (const k of Object.keys(obj)) {
      const f = obj[k];
      if (typeof f !== "function" || f.__counted || k === "lastEventsJson") continue;
      const key = `${name}.${k}`;
      const g = (...a) => {
        counts[key] = (counts[key] ?? 0) + 1;
        return f(...a);
      };
      g.__counted = true;
      obj[k] = g;
    }
  }
});

const cdp = await page.context().newCDPSession(page);
await cdp.send("HeapProfiler.enable");
await cdp.send("HeapProfiler.collectGarbage");
await page.evaluate(() => {
  const wp = window.__wp;
  for (const k of Object.keys(window.__wasmCalls)) delete window.__wasmCalls[k];
  wp.app.prof.resetAcc();
  window.__probe = { tick0: wp.sim.worldInfo()[0], t0: performance.now(), calls: 0, tris: 0, n: 0, voices0: window.__wpAudio?.pool?.created ?? 0 };
  // Draw calls / triangles of every rendered frame (renderer.info resets per render).
  const r = wp.scene.renderer;
  const orig = r.render.bind(r);
  r.render = (s, c) => {
    orig(s, c);
    const p = window.__probe;
    p.calls += r.info.render.calls;
    p.tris += r.info.render.triangles;
    p.n++;
  };
  window.__probe.restore = () => (r.render = orig);
  // Wall frame intervals (rAF), for the distribution: frames over 250 ms lose sim time.
  const ivs = (window.__probe.intervals = []);
  let last = performance.now();
  const tick = (t) => {
    ivs.push(t - last);
    last = t;
    if (window.__probe.intervals === ivs) requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
});
await cdp.send("HeapProfiler.startSampling", {
  samplingInterval: 8192,
  includeObjectsCollectedByMajorGC: true,
  includeObjectsCollectedByMinorGC: true,
});
await page.waitForTimeout(Number(secs) * 1000);
const { profile } = await cdp.send("HeapProfiler.stopSampling");
const r = await page.evaluate(() => {
  const wp = window.__wp;
  const p = window.__probe;
  p.restore();
  const wall = (performance.now() - p.t0) / 1000;
  const iv = p.intervals.slice(1).sort((a, b) => a - b);
  p.intervals = null;
  const q = (x) => iv[Math.min(iv.length - 1, Math.floor(x * iv.length))] ?? 0;
  const prof = wp.app.prof;
  const frames = prof.frames;
  const sections = Object.fromEntries(Object.entries(prof.acc).map(([k, v]) => [k, v / frames]));
  const sum = Object.values(sections).reduce((a, b) => a + b, 0);
  const calls = Object.entries(window.__wasmCalls).sort((a, b) => b[1] - a[1]);
  const totalCalls = calls.reduce((a, [, n]) => a + n, 0);
  // One GPU-synced render of the current view (SwiftShader raster included).
  wp.app.paused = true;
  const ren = wp.scene.renderer;
  const gl = ren.getContext();
  const px = new Uint8Array(4);
  const ts = [];
  for (let i = 0; i < 4; i++) {
    const t0 = performance.now();
    ren.render(wp.scene.scene, wp.rig.camera);
    gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
    ts.push(performance.now() - t0);
  }
  ts.sort((a, b) => a - b);
  // Structure (machine-independent): point lights, city tiles, signal packing, and material
  // version bumps over steady renders (a bump re-resolves the shader program every frame).
  const mats = new Set();
  let pointLights = 0;
  wp.scene.scene.traverse((o) => {
    if (o.isPointLight) pointLights++;
    for (const m of [o.material].flat()) if (m) mats.add(m);
  });
  const ver = () => [...mats].reduce((a, m) => a + m.version, 0);
  const v0 = ver();
  const before = new Map([...mats].map((m) => [m, m.version]));
  for (let i = 0; i < 3; i++) ren.render(wp.scene.scene, wp.rig.camera);
  // Which materials bumped (debugging a churn failure): type and name.
  const churned = [...mats].filter((m) => m.version !== before.get(m)).map((m) => `${m.type}:${m.name || "?"}`);
  // (Optional chaining: the same script also profiles the pre-pass build for before/after.)
  const city = wp.scene.cityStats?.(wp.rig.camera) ?? { tiles: 0, drawn: 0 };
  const structure = {
    pointLights,
    cityTiles: city.tiles,
    cityTilesDrawn: city.drawn,
    signalHeads: wp.scene.signals.count,
    signalsDrawn: wp.scene.signals.drawnCount ?? wp.scene.signals.count,
    programChurn: ver() - v0,
    churned: churned.slice(0, 8),
    // Crash effects (render/fx): quality, particles spawned in the window, draw calls now.
    fx: wp.fx?.stats() ?? null,
  };
  wp.app.paused = false;
  return {
    frames,
    wallS: wall,
    fps: frames / wall,
    rtf: (wp.sim.worldInfo()[0] - p.tick0) / 240 / wall,
    frameMs: (wall * 1000) / frames,
    frameP50: q(0.5),
    frameP90: q(0.9),
    frameMax: iv[iv.length - 1] ?? 0,
    framesOver250: iv.filter((x) => x > 250).length,
    sections,
    sectionsSum: sum,
    rasterWaitMs: (wall * 1000) / frames - sum,
    syncedRenderMs: ts[1],
    drawCalls: p.calls / Math.max(p.n, 1),
    triangles: p.tris / Math.max(p.n, 1),
    programs: ren.info.programs?.length ?? 0,
    geometries: ren.info.memory.geometries,
    wasmCallsPerFrame: totalCalls / frames,
    wasmTop: calls.slice(0, 14).map(([k, n]) => [k, +(n / frames).toFixed(2)]),
    vehicles: wp.sim.vehicleMeta().length / 3,
    gfx: wp.scene.quality?.label() ?? "pre-pass (MSAA, native res)",
    structure,
    agents: wp.sim.trafficAgents().length / 5,
    audioRunning: window.__wpAudio?.ctx?.state === "running",
    audioVoicesPerS: ((window.__wpAudio?.pool?.created ?? 0) - p.voices0) / wall,
    audioStripsReused: window.__wpAudio?.pool?.stripsReused ?? 0,
    audioMs: window.__wpAudio?.frameMs ?? 0,
    speedKmh: wp.sim.states()[14] * 3.6,
  };
});
// Allocation profile: total sampled bytes and the top allocating functions (self).
let total = 0;
const byFn = new Map();
const walk = (n) => {
  total += n.selfSize;
  const cf = n.callFrame;
  const key = `${cf.functionName || "(anon)"} ${cf.url.split("/").pop().split("?")[0]}:${cf.lineNumber + 1}`;
  byFn.set(key, (byFn.get(key) ?? 0) + n.selfSize);
  for (const c of n.children) walk(c);
};
walk(profile.head);
// STACKS=<function name>: print the call paths that allocate in that function (debugging).
if (process.env.STACKS) {
  const paths = new Map();
  const visit = (n, stack) => {
    const cf = n.callFrame;
    const here = [...stack, `${cf.functionName || "(anon)"}:${cf.lineNumber + 1}`];
    if (cf.functionName === process.env.STACKS) {
      const k = here.slice(-7).join(" < ".replace("<", ">"));
      paths.set(k, (paths.get(k) ?? 0) + n.selfSize);
    }
    for (const c of n.children) visit(c, here);
  };
  visit(profile.head, []);
  for (const [k, v] of [...paths].sort((a, b) => b[1] - a[1]).slice(0, 8)) console.log(`${(v / 1024).toFixed(0)} KB  ${k}`);
}
const kbPerFrame = total / 1024 / r.frames;
const allocTop = [...byFn].sort((a, b) => b[1] - a[1]).slice(0, 12).map(([k, v]) => [k, +(v / 1024 / r.frames).toFixed(1)]);
const out = { which, viewport: `${vw}x${vh}`, ...r, allocKBPerFrame: kbPerFrame, allocTop, errors: errors.slice(0, 3) };
if (jsonOnly) console.log(JSON.stringify(out));
else {
  const f = (x, d = 1) => (typeof x === "number" ? x.toFixed(d) : String(x));
  console.log(`== ${which} @ ${out.viewport}: ${r.frames} frames in ${f(r.wallS)} s · ${f(r.fps, 2)} fps · RTF ${f(r.rtf, 3)} · audio ${r.audioRunning ? "on" : "off"}`);
  console.log(`frame p50 ${f(r.frameP50)} · p90 ${f(r.frameP90)} · max ${f(r.frameMax)} ms · ${r.framesOver250} frames > 250 ms (lose sim time)`);
  console.log(`frame ${f(r.frameMs)} ms = sections ${f(r.sectionsSum)} + raster/compositor wait ${f(r.rasterWaitMs)} · GPU-synced render ${f(r.syncedRenderMs)} ms`);
  console.log("sections ms/frame: " + Object.entries(r.sections).map(([k, v]) => `${k} ${f(v, 2)}`).join(" · "));
  console.log(`audio: ${f(r.audioVoicesPerS)} voices created/s (${r.audioStripsReused} node chains reused so far), ${f(r.audioMs, 2)} ms/frame · quality ${r.gfx}`);
  console.log("structure: " + JSON.stringify(r.structure));
  console.log(`draw calls ${f(r.drawCalls, 0)} · triangles ${f(r.triangles, 0)} · programs ${r.programs} · vehicles ${r.vehicles} · agents ${r.agents}`);
  console.log(`wasm calls/frame ${f(r.wasmCallsPerFrame)}: ` + r.wasmTop.map(([k, n]) => `${k} ${n}`).join(", "));
  console.log(`alloc ${f(kbPerFrame)} KB/frame; top: ` + allocTop.map(([k, v]) => `${k} ${v}`).join(" | "));
  if (out.errors.length) console.log("errors:", out.errors);
  console.log(JSON.stringify(out));
}
await browser.close();
process.exit(0);
