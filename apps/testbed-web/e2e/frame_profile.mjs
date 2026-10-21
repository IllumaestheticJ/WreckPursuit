// Browser frame profile of the city chase (browser perf pass, docs/integration/browser-perf.md).
// Usage: node e2e/frame_profile.mjs [chase|rain] [seconds]   (needs `npm run wasm`; own Vite)
// Env: VIEWPORT=WxH (default 1280x720), CITY=<id> (default the app's default, vesper_v2),
// QUERY=<url params> (e.g. gfx=high), EVAL=<js> run before measuring, STACKS=<fn> allocation paths,
// JSON=1 prints only the JSON line. Phone: TOUCH=1 emulates a touch device (hasTouch, isMobile,
// DPR 3: the touch graphics profile), CPU_THROTTLE=<n> slows the CPU n× (CDP), e.g.
// `VIEWPORT=740x360 TOUCH=1 CPU_THROTTLE=4 node e2e/frame_profile.mjs chase` (docs/integration/touch.md).
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
    // JS_FLAGS=<v8 flags>: e.g. "--trace-deopt" to see why a hot loop stays in a boxing tier.
    ...(process.env.JS_FLAGS ? [`--js-flags=${process.env.JS_FLAGS}`] : []),
  ],
});
const [vw, vh] = (process.env.VIEWPORT ?? "1280x720").split("x").map(Number);
const touchDevice = process.env.TOUCH === "1";
const page = await (
  await browser.newContext({ viewport: { width: vw, height: vh }, ...(touchDevice ? { hasTouch: true, isMobile: true, deviceScaleFactor: 3 } : {}) })
).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 90000 });
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

// Draw calls / triangles of every rendered frame (renderer.info resets per render). The
// low-poly post stack (render/post.ts) calls render() several times a frame: every call
// adds its draws, but only the main scene's render counts as a frame.
// Installed before the chase starts, like the wasm call counters above: replacing a renderer
// method (or a `sim` function) changes a constant field, which throws away the optimised code
// of every caller (renderObject, the frame loop); done just before measuring, the window then
// measured that code re-warming in a boxing tier (allocation pass 2026-10-20). Counters are
// typed arrays, so the probe itself does not allocate per draw.
await page.evaluate(() => {
  const wp = window.__wp;
  const r = wp.scene.renderer;
  const orig = r.render.bind(r);
  // Per-feature draw calls: every draw goes through renderBufferDirect; the drawn object's
  // nearest parent tagged with userData.perf (src/perf.ts tagFeature) names the feature.
  // Draws of other scenes (the post stack's full-screen passes) count as "post".
  const names = [];
  const index = new Map();
  const calls = new Float64Array(64);
  const tris = new Float64Array(64);
  const slot = (name) => {
    let i = index.get(name);
    if (i === undefined) {
      i = names.length;
      names.push(name);
      index.set(name, i);
    }
    return i;
  };
  const POST = slot("post");
  const tagOf = new WeakMap();
  const featureOf = (o) => {
    let t = tagOf.get(o);
    if (t !== undefined) return t;
    let name = "other";
    for (let x = o; x; x = x.parent)
      if (x.userData?.perf) {
        name = x.userData.perf;
        break;
      }
    t = slot(name);
    tagOf.set(o, t);
    return t;
  };
  let inMain = false;
  const totals = new Float64Array(3); // draw calls, triangles, main-scene renders
  window.__probeFeat = {
    reset() {
      calls.fill(0);
      tris.fill(0);
      totals.fill(0);
    },
    get calls() {
      return totals[0];
    },
    get tris() {
      return totals[1];
    },
    get n() {
      return totals[2];
    },
    feat: () => Object.fromEntries(names.map((k, i) => [k, calls[i]]).filter(([, v]) => v > 0)),
    featTris: () => Object.fromEntries(names.map((k, i) => [k, tris[i]]).filter(([, v]) => v > 0)),
  };
  const origRbd = r.renderBufferDirect.bind(r);
  r.renderBufferDirect = (camera, scene, geometry, material, object, group) => {
    const k = inMain ? featureOf(object) : POST;
    calls[k] += 1;
    // Triangles (approx.: the draw range ignored): index or vertex count / 3 × instances.
    const n = geometry.index ? geometry.index.count : (geometry.attributes.position?.count ?? 0);
    tris[k] += (object.isMesh ? n / 3 : 0) * (object.isInstancedMesh ? object.count : 1);
    return origRbd(camera, scene, geometry, material, object, group);
  };
  r.render = (s, c) => {
    inMain = s === wp.scene.scene;
    orig(s, c);
    inMain = false;
    totals[0] += r.info.render.calls;
    totals[1] += r.info.render.triangles;
    if (s === wp.scene.scene) totals[2] += 1;
  };
  window.__probeRestore = () => {
    r.render = orig;
    r.renderBufferDirect = origRbd;
  };
});
await page.evaluate((rain) => window.__wp.startCityChase(undefined, "Arcade", rain), which === "rain");
// The key press also starts the AudioContext (recorded packs load in the warm-up).
await page.keyboard.down("KeyW");
// WARMUP_S=<s>: chase time before measuring (default 5). At 3–4 fps much of the JIT warm-up of the
// frame loop (Three's render path, the app's per-car loops) is still going on after 5 s.
await page.waitForTimeout(Number(process.env.WARMUP_S ?? 5) * 1000);
// EVAL=<js>: run in the page before measuring (A/B experiments, e.g. hide a layer).
if (process.env.EVAL) await page.evaluate(process.env.EVAL);

const cdp = await page.context().newCDPSession(page);
if (Number(process.env.CPU_THROTTLE) > 1) await cdp.send("Emulation.setCPUThrottlingRate", { rate: Number(process.env.CPU_THROTTLE) });
await cdp.send("HeapProfiler.enable");
await cdp.send("HeapProfiler.collectGarbage");
await page.evaluate(() => {
  const wp = window.__wp;
  for (const k of Object.keys(window.__wasmCalls)) delete window.__wasmCalls[k];
  wp.app.prof.resetAcc();
  const PB = [...wp.app.views.values()][0]?.panels.constructor;
  window.__probe = { PB, reskins0: PB?.reskins ?? 0, views0: wp.app.views.size, tick0: wp.sim.worldInfo()[0], t0: performance.now(), voices0: window.__wpAudio?.pool?.created ?? 0 };
  window.__probeFeat.reset();
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
// PROFILE_OUT=<file>: also save the raw sampling heap profile (offline analysis).
if (process.env.PROFILE_OUT) (await import("node:fs")).writeFileSync(process.env.PROFILE_OUT, JSON.stringify(profile));
const r = await page.evaluate(() => {
  const wp = window.__wp;
  const p = window.__probe;
  window.__probeRestore();
  const pf = window.__probeFeat;
  const wall = (performance.now() - p.t0) / 1000;
  const iv = p.intervals.slice(1).sort((a, b) => a - b);
  p.intervals = null;
  const q = (x) => iv[Math.min(iv.length - 1, Math.floor(x * iv.length))] ?? 0;
  const prof = wp.app.prof;
  const frames = prof.frames;
  const sections = Object.fromEntries(Object.entries(prof.acc).map(([k, v]) => [k, v / frames]));
  // Per section: worst frame, and the mean without it (one GC pause or compile moves a mean).
  const peaks = Object.fromEntries(Object.entries(prof.peak ?? {}).map(([k, v]) => [k, v]));
  const sectionsTrim = Object.fromEntries(Object.entries(prof.acc).map(([k, v]) => [k, frames > 1 ? (v - (prof.peak?.[k] ?? 0)) / (frames - 1) : v]));
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
    wp.scene.render(wp.rig.camera); // through the art style's post stack when it is on
    gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
    ts.push(performance.now() - t0);
  }
  ts.sort((a, b) => a - b);
  // Raster cost per feature (A/B): the GPU-synced render with that feature's roots hidden;
  // the difference is what the feature costs (CPU submit + SwiftShader raster). "post" = the
  // style's post stack vs a plain render.
  const synced = () => {
    const t = [];
    for (let i = 0; i < 3; i++) {
      const t0 = performance.now();
      wp.scene.render(wp.rig.camera);
      gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
      t.push(performance.now() - t0);
    }
    return t.sort((a, b) => a - b)[1];
  };
  const roots = {};
  wp.scene.scene.traverse((o) => {
    const f = o.userData?.perf;
    if (f && o.visible) (roots[f] ??= []).push(o);
  });
  const base = synced();
  const featureMs = {};
  for (const [f, objs] of Object.entries(roots)) {
    for (const o of objs) o.visible = false;
    featureMs[f] = Math.max(0, base - synced());
    for (const o of objs) o.visible = true;
  }
  if (wp.scene.renderOverride) {
    const ov = wp.scene.renderOverride;
    wp.scene.renderOverride = null;
    featureMs.post = Math.max(0, base - synced());
    wp.scene.renderOverride = ov;
  }
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
  for (let i = 0; i < 3; i++) wp.scene.render(wp.rig.camera);
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
    style: wp.style?.id ?? "greybox",
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
    sectionsTrim,
    peaks,
    sectionsSum: sum,
    rasterWaitMs: (wall * 1000) / frames - sum,
    syncedRenderMs: ts[1],
    drawCalls: pf.calls / Math.max(pf.n, 1),
    // Draw calls per frame by feature (incl. the post passes) and raster ms per feature (A/B).
    featureCalls: Object.fromEntries(Object.entries(pf.feat()).sort((a, b) => b[1] - a[1]).map(([k, v]) => [k, +(v / Math.max(pf.n, 1)).toFixed(1)])),
    featureKTris: Object.fromEntries(Object.entries(pf.featTris()).sort((a, b) => b[1] - a[1]).map(([k, v]) => [k, +(v / Math.max(pf.n, 1) / 1000).toFixed(1)])),
    featureMs: Object.fromEntries(Object.entries(featureMs).sort((a, b) => b[1] - a[1]).map(([k, v]) => [k, +v.toFixed(1)])),
    triangles: pf.tris / Math.max(pf.n, 1),
    programs: ren.info.programs?.length ?? 0,
    geometries: ren.info.memory.geometries,
    wasmCallsPerFrame: totalCalls / frames,
    wasmTop: calls.slice(0, 14).map(([k, n]) => [k, +(n / frames).toFixed(2)]),
    vehicles: wp.sim.vehicleMeta().length / 3,
    gfx: wp.scene.quality?.label() ?? "pre-pass (MSAA, native res)",
    structure,
    agents: wp.sim.trafficAgents().length / 7,
    audioRunning: window.__wpAudio?.ctx?.state === "running",
    audioVoicesPerS: ((window.__wpAudio?.pool?.created ?? 0) - p.voices0) / wall,
    audioStripsReused: window.__wpAudio?.pool?.stripsReused ?? 0,
    audioMs: window.__wpAudio?.frameMs ?? 0,
    speedKmh: wp.sim.states()[14] * 3.6,
    // Panel re-skins per frame (crashed cars whose lattice moved; render/panels.ts).
    reskinsPerFrame: ((p.PB?.reskins ?? 0) - p.reskins0) / frames,
  };
});
// Allocation profile: total sampled bytes and the top allocating functions (self). Bytes under
// one-off work are counted apart as "transient": building a car view when traffic / police are
// promoted (the VehicleView constructor), and a material's first shader-program binding or compile (getProgram,
// prewarm), the audio noise tables (once per AudioContext). The rest is the steady per-frame cost of the loop.
// noiseBuffers: the audio noise tables, built once per AudioContext on the first impact sound.
const TRANSIENT = new Set(["VehicleView", "getProgram", "prewarm", "acquireProgram", "onFirstUse", "noiseBuffers"]);
let total = 0;
let transient = 0;
const byFn = new Map();
const steadyByFn = new Map();
const walk = (n, inTransient) => {
  const cf = n.callFrame;
  const tr = inTransient || TRANSIENT.has(cf.functionName) || cf.functionName.endsWith(".prewarm");
  total += n.selfSize;
  if (tr) transient += n.selfSize;
  const key = `${cf.functionName || "(anon)"} ${cf.url.split("/").pop().split("?")[0]}:${cf.lineNumber + 1}`;
  byFn.set(key, (byFn.get(key) ?? 0) + n.selfSize);
  if (!tr) steadyByFn.set(key, (steadyByFn.get(key) ?? 0) + n.selfSize);
  for (const c of n.children) walk(c, tr);
};
walk(profile.head, false);
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
const steadyKBPerFrame = (total - transient) / 1024 / r.frames;
// TOP=<n>: length of the steady top list (default 12).
const steadyTop = [...steadyByFn].sort((a, b) => b[1] - a[1]).slice(0, Number(process.env.TOP ?? 12)).map(([k, v]) => [k, +(v / 1024 / r.frames).toFixed(1)]);
const allocTop = [...byFn].sort((a, b) => b[1] - a[1]).slice(0, 12).map(([k, v]) => [k, +(v / 1024 / r.frames).toFixed(1)]);
const out = { which, viewport: `${vw}x${vh}`, ...r, allocKBPerFrame: kbPerFrame, steadyAllocKBPerFrame: steadyKBPerFrame, allocTop, steadyTop, errors: errors.slice(0, 3) };
if (jsonOnly) console.log(JSON.stringify(out));
else {
  const f = (x, d = 1) => (typeof x === "number" ? x.toFixed(d) : String(x));
  console.log(`== ${which} @ ${out.viewport}: ${r.frames} frames in ${f(r.wallS)} s · ${f(r.fps, 2)} fps · RTF ${f(r.rtf, 3)} · audio ${r.audioRunning ? "on" : "off"}`);
  console.log(`frame p50 ${f(r.frameP50)} · p90 ${f(r.frameP90)} · max ${f(r.frameMax)} ms · ${r.framesOver250} frames > 250 ms (lose sim time)`);
  console.log(`frame ${f(r.frameMs)} ms = sections ${f(r.sectionsSum)} + raster/compositor wait ${f(r.rasterWaitMs)} · GPU-synced render ${f(r.syncedRenderMs)} ms`);
  console.log("sections ms/frame: " + Object.entries(r.sections).map(([k, v]) => `${k} ${f(v, 2)}`).join(" · "));
  console.log(`audio: ${f(r.audioVoicesPerS)} voices created/s (${r.audioStripsReused} node chains reused so far), ${f(r.audioMs, 2)} ms/frame · quality ${r.gfx}`);
  console.log("structure: " + JSON.stringify(r.structure));
  console.log("draw calls by feature: " + Object.entries(r.featureCalls).map(([k, v]) => `${k} ${v}`).join(" · "));
  console.log("k triangles by feature: " + Object.entries(r.featureKTris).map(([k, v]) => `${k} ${v}`).join(" · "));
  console.log("synced render cost by feature (hide A/B, ms): " + Object.entries(r.featureMs).map(([k, v]) => `${k} ${v}`).join(" · "));
  console.log(`panel re-skins / frame ${f(r.reskinsPerFrame, 2)}`);
  console.log(`draw calls ${f(r.drawCalls, 0)} · triangles ${f(r.triangles, 0)} · programs ${r.programs} · vehicles ${r.vehicles} · agents ${r.agents}`);
  console.log(`wasm calls/frame ${f(r.wasmCallsPerFrame)}: ` + r.wasmTop.map(([k, n]) => `${k} ${n}`).join(", "));
  console.log(`alloc ${f(kbPerFrame)} KB/frame (steady ${f(steadyKBPerFrame)}, one-off view / shader builds ${f(kbPerFrame - steadyKBPerFrame)}); top: ` + allocTop.map(([k, v]) => `${k} ${v}`).join(" | "));
  console.log(`steady alloc top: ` + steadyTop.map(([k, v]) => `${k} ${v}`).join(" | "));
  if (out.errors.length) console.log("errors:", out.errors);
  console.log(JSON.stringify(out));
}
await browser.close();
process.exit(0);
