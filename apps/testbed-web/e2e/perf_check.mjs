// Browser perf regression check (docs/integration/browser-perf.md): runs e2e/frame_profile.mjs on
// the night run and the rainy-night chase (1280×720) and on the phone profile (740×360 touch,
// chase), all at the default "auto" graphics quality (= the software-GL / touch profiles under
// headless SwiftShader), and fails on the targets below. Every run also prints per-feature cost
// lines (frame-loop sections, draw calls, triangles and raster ms per render feature, steady vs
// one-off allocation) so a regression is attributable to the feature that caused it.
// Usage: node e2e/perf_check.mjs (needs `npm run wasm`). Run by e2e/run.mjs (`just web-e2e`;
// PERF=0 skips it).
//
// Machine speed: real-time factor and slow frames depend on the box (headless SwiftShader is a
// CPU rasterizer). On a box where even the pre-round-12b build stays under the targets,
// PERF_MIN_RTF / PERF_MAX_SLOW override those two (the doc lists the calibration runs); every
// other check is machine-independent or generous.
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const SECS = process.env.PERF_SECS ?? "12";

/** Targets (round 12b perf pass). */
const LIMITS = {
  minRtf: Number(process.env.PERF_MIN_RTF ?? 0.9),
  // Every frame over 250 ms loses sim time (slow motion).
  maxFramesOver250: Number(process.env.PERF_MAX_SLOW ?? 12),
  // Allocation of the frame loop itself, KB per frame (V8 sampling heap profiler, collected
  // objects included = GC pressure), without one-off work: building a car view when traffic /
  // police are promoted, a material's first shader program (frame_profile.mjs `steadyAlloc…`).
  maxSteadyAllocKBPerFrame: 200,
  // Everything (one-off included); measured ~1000–1400 KB after the 12b pass, 2500–2800 before.
  maxAllocKBPerFrame: 2400,
  // Desktop: measured ~300 / ~280 after the 12b pass (774 before); the phone has its own limit.
  maxDrawCalls: 600,
  maxPhoneDrawCalls: 300,
  // Measured ~200k after the 12b pass (~456k before).
  maxTriangles: 400000,
  // Measured ~45 (most of it: panel states + lattice nodes per car).
  maxWasmCallsPerFrame: 120,
  // Crash effects (render/fx, "low" on this profile): CPU per frame incl. emitters and upload
  // prep, mean without the single worst frame (a GC pause in that lap moved the mean to 1.8);
  // measured 0.3–0.6 ms at ~3–4 fps. One draw call per effect kind.
  maxFxMsPerFrame: 1.0,
  maxFxDrawCalls: 5,
};

const RUNS = [
  { which: "chase", env: {} },
  { which: "rain", env: {} },
  // Phone (docs/integration/touch.md): landscape 740×360, touch (DPR 3, the touch profile).
  { which: "chase", env: { VIEWPORT: "740x360", TOUCH: "1" }, phone: true },
];

let failures = 0;
const check = (ok, msg) => {
  console.log(`${ok ? "PASS" : "FAIL"}  ${msg}`);
  if (!ok) failures++;
};
const info = (msg) => console.log(`INFO  ${msg}`);
const fmt = (o, d = 1, unit = "") =>
  Object.entries(o)
    .map(([k, v]) => `${k} ${typeof v === "number" ? v.toFixed(d) : v}${unit}`)
    .join(" · ");

for (const run of RUNS) {
  const r = spawnSync("node", [join(here, "frame_profile.mjs"), run.which, SECS], {
    cwd: join(here, ".."),
    // The art style's default (low-poly dark, render/style.ts) is what players get: measure it.
    env: { ...process.env, ...run.env, JSON: "1", QUERY: process.env.QUERY ?? "style=lowpoly" },
    encoding: "utf8",
    timeout: 300000,
  });
  const line = (r.stdout ?? "").trim().split("\n").filter((l) => l.startsWith("{")).pop();
  const name = `${run.phone ? "phone " : ""}${run.which}`;
  if (!line) {
    check(false, `perf ${name}: profile did not run (${(r.stderr ?? "").slice(-400)})`);
    continue;
  }
  const p = JSON.parse(line);
  const tag = `perf ${name} @ ${p.viewport} [${p.gfx}, ${p.style}]`;
  check(p.errors.length === 0, `${tag}: no page errors (${p.errors.join(" | ")})`);
  check(p.rtf >= LIMITS.minRtf, `${tag}: real-time factor ${p.rtf.toFixed(3)} ≥ ${LIMITS.minRtf} · ${p.fps.toFixed(1)} fps · p50 ${p.frameP50.toFixed(0)} ms · p90 ${p.frameP90.toFixed(0)} ms`);
  check(p.framesOver250 <= LIMITS.maxFramesOver250, `${tag}: ${p.framesOver250} frames over 250 ms (≤ ${LIMITS.maxFramesOver250})`);
  const maxCalls = run.phone ? LIMITS.maxPhoneDrawCalls : LIMITS.maxDrawCalls;
  check(p.drawCalls < maxCalls, `${tag}: ${p.drawCalls.toFixed(0)} draw calls / frame (< ${maxCalls})`);
  check(p.triangles <= LIMITS.maxTriangles, `${tag}: ${p.triangles.toFixed(0)} triangles / frame (≤ ${LIMITS.maxTriangles})`);
  check(p.wasmCallsPerFrame <= LIMITS.maxWasmCallsPerFrame, `${tag}: ${p.wasmCallsPerFrame.toFixed(1)} wasm calls / frame (≤ ${LIMITS.maxWasmCallsPerFrame})`);
  check(
    p.steadyAllocKBPerFrame <= LIMITS.maxSteadyAllocKBPerFrame && p.allocKBPerFrame <= LIMITS.maxAllocKBPerFrame,
    `${tag}: ${p.steadyAllocKBPerFrame.toFixed(0)} KB allocated / frame steady (≤ ${LIMITS.maxSteadyAllocKBPerFrame}), ` +
      `${p.allocKBPerFrame.toFixed(0)} KB with one-off view / shader builds (≤ ${LIMITS.maxAllocKBPerFrame})`,
  );
  // Structural checks (machine-independent): the fixes stay in place.
  const s = p.structure;
  check(s.pointLights <= 1, `${tag}: ${s.pointLights} point light(s) in the scene (one shared siren light)`);
  check(s.cityTiles > 50 && s.cityTilesDrawn < s.cityTiles, `${tag}: city drawn as ${s.cityTiles} tiles, ${s.cityTilesDrawn} drawn this frame`);
  check(s.signalHeads === 0 || s.signalsDrawn < s.signalHeads, `${tag}: ${s.signalsDrawn} of ${s.signalHeads} signal heads drawn (near the camera)`);
  if (s.fx) {
    const fxMs = p.sectionsTrim?.fx ?? p.sections.fx ?? 0;
    check(
      fxMs <= LIMITS.maxFxMsPerFrame && s.fx.drawCalls <= LIMITS.maxFxDrawCalls,
      `${tag}: crash effects [${s.fx.quality}] ${fxMs.toFixed(3)} ms/frame (≤ ${LIMITS.maxFxMsPerFrame}), ${s.fx.drawCalls} draw calls (≤ ${LIMITS.maxFxDrawCalls}); ` +
        `${s.fx.sparks} sparks, ${s.fx.smoke} puffs, ${s.fx.shards} shards, ${s.fx.chips} chips, ${s.fx.skidSegments} skid segments this session`,
    );
  }
  check(s.programChurn === 0, `${tag}: no shader program re-resolve in steady frames (${s.programChurn} material version bumps over 3 renders${s.churned?.length ? ": " + s.churned.join(", ") : ""})`);
  // Per-feature cost lines (attribution, not checked).
  const sections = Object.fromEntries(Object.entries(p.sections).filter(([, v]) => v >= 0.05).sort((a, b) => b[1] - a[1]));
  info(`${tag}: frame ${p.frameMs.toFixed(0)} ms = JS sections ${p.sectionsSum.toFixed(0)} + raster / compositor ${p.rasterWaitMs.toFixed(0)}; GPU-synced render ${p.syncedRenderMs.toFixed(0)} ms`);
  info(`${tag}: JS ms/frame by section: ${fmt(sections, 2)}`);
  const peaks = Object.fromEntries(Object.entries(p.peaks ?? {}).filter(([, v]) => v >= 5).sort((a, b) => b[1] - a[1]));
  info(`${tag}: worst single frame by section, ms: ${fmt(peaks, 0)}`);
  info(`${tag}: draw calls by feature: ${fmt(p.featureCalls, 0)}`);
  info(`${tag}: k triangles by feature: ${fmt(p.featureKTris, 1)}`);
  info(`${tag}: raster ms by feature (synced render with it hidden, A/B; ±10 ms noise): ${fmt(p.featureMs, 0)}`);
  info(`${tag}: panel re-skins ${p.reskinsPerFrame.toFixed(2)} / frame; top allocators KB/frame: ${p.allocTop.slice(0, 6).map(([k, v]) => `${k} ${v}`).join(" | ")}`);
}

console.log(failures === 0 ? "\nperf check passed" : `\n${failures} perf check(s) FAILED`);
process.exit(failures === 0 ? 0 : 1);
