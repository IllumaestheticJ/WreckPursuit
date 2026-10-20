// Browser perf regression check (browser perf pass, docs/integration/browser-perf.md): runs
// e2e/frame_profile.mjs on the city chase and the rainy-night chase (1280×720, default "auto"
// graphics quality = the software-GL profile under headless SwiftShader) and fails on a big
// regression. Thresholds are deliberately generous (≈2× the numbers measured when this was
// written, see the doc) so a slow CI box does not flake; the target real-time factor (0.95) is
// printed, the hard floor is lower. Usage: node e2e/perf_check.mjs (needs `npm run wasm`).
// Run by e2e/run.mjs (`just web-e2e`) at the end.
import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const SECS = process.env.PERF_SECS ?? "12";

/** Hard limits per run (fail) and the target real-time factor (reported). */
const LIMITS = {
  // Measured after the pass: 0.99–1.00 (chase), 0.97–0.99 (rain). Before: 0.23 / 0.2x.
  minRtf: 0.75,
  targetRtf: 0.95,
  // Measured ~560 / ~520; before ~355 (one mesh per city material, nothing culled).
  maxDrawCalls: 1200,
  // Measured ~145k / ~110k; before ~522k.
  maxTriangles: 300000,
  // Measured ~51 / ~52 (most of it: telemetry + panel states + lattice nodes per car).
  maxWasmCallsPerFrame: 120,
  // Measured ~550 / ~680 KB (the sampling profiler counts collected objects: GC pressure).
  maxAllocKBPerFrame: 1600,
  // Every frame over 250 ms loses sim time (slow motion); measured 0–3 in 12 s.
  maxFramesOver250: 12,
};

let failures = 0;
const check = (ok, msg) => {
  console.log(`${ok ? "PASS" : "FAIL"}  ${msg}`);
  if (!ok) failures++;
};

for (const which of ["chase", "rain"]) {
  const r = spawnSync("node", [join(here, "frame_profile.mjs"), which, SECS], {
    cwd: join(here, ".."),
    env: { ...process.env, JSON: "1", STRUCT: "1" },
    encoding: "utf8",
    timeout: 300000,
  });
  const line = (r.stdout ?? "").trim().split("\n").filter((l) => l.startsWith("{")).pop();
  if (!line) {
    check(false, `perf ${which}: profile did not run (${(r.stderr ?? "").slice(-400)})`);
    continue;
  }
  const p = JSON.parse(line);
  const tag = `perf ${which} @ ${p.viewport} [${p.gfx}]`;
  check(p.errors.length === 0, `${tag}: no page errors (${p.errors.join(" | ")})`);
  check(
    p.rtf >= LIMITS.minRtf,
    `${tag}: real-time factor ${p.rtf.toFixed(3)} ≥ ${LIMITS.minRtf} (target ${LIMITS.targetRtf}${p.rtf >= LIMITS.targetRtf ? ", met" : ", NOT met"}) · ${p.fps.toFixed(1)} fps · p50 ${p.frameP50.toFixed(0)} ms`,
  );
  check(p.framesOver250 <= LIMITS.maxFramesOver250, `${tag}: ${p.framesOver250} frames over 250 ms (≤ ${LIMITS.maxFramesOver250})`);
  check(p.drawCalls <= LIMITS.maxDrawCalls, `${tag}: ${p.drawCalls.toFixed(0)} draw calls / frame (≤ ${LIMITS.maxDrawCalls})`);
  check(p.triangles <= LIMITS.maxTriangles, `${tag}: ${p.triangles.toFixed(0)} triangles / frame (≤ ${LIMITS.maxTriangles})`);
  check(
    p.wasmCallsPerFrame <= LIMITS.maxWasmCallsPerFrame,
    `${tag}: ${p.wasmCallsPerFrame.toFixed(1)} wasm calls / frame (≤ ${LIMITS.maxWasmCallsPerFrame})`,
  );
  check(
    p.allocKBPerFrame <= LIMITS.maxAllocKBPerFrame,
    `${tag}: ${p.allocKBPerFrame.toFixed(0)} KB allocated / frame (≤ ${LIMITS.maxAllocKBPerFrame})`,
  );
  // Structural checks (machine-independent): the fixes stay in place.
  const s = p.structure;
  check(s.pointLights <= 1, `${tag}: ${s.pointLights} point light(s) in the scene (one shared siren light)`);
  check(s.cityTiles > 50 && s.cityTilesDrawn < s.cityTiles, `${tag}: city drawn as ${s.cityTiles} tiles, ${s.cityTilesDrawn} drawn this frame`);
  check(s.signalHeads === 0 || s.signalsDrawn < s.signalHeads, `${tag}: ${s.signalsDrawn} of ${s.signalHeads} signal heads drawn (near the camera)`);
  check(s.programChurn === 0, `${tag}: no shader program re-resolve in steady frames (${s.programChurn} material version bumps over 3 renders)`);
}

console.log(failures === 0 ? "\nperf check passed" : `\n${failures} perf check(s) FAILED`);
process.exit(failures === 0 ? 0 : 1);
