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
  // Measured after the pass (with round 12's fleet / LOD traffic merged): 0.95–0.99 (chase),
  // 0.98–0.99 (rain). Before the pass: 0.25 / 0.22.
  minRtf: 0.75,
  targetRtf: 0.95,
  // Measured ~755 / ~505; before ~355 (one mesh per city material, nothing culled).
  maxDrawCalls: 1600,
  // Measured ~340k / ~265k with the fleet meshes (~145k / ~113k without); before ~522k.
  maxTriangles: 700000,
  // Measured ~54 / ~44 (most of it: panel states + lattice nodes per car).
  maxWasmCallsPerFrame: 120,
  // Measured ~630–1120 KB (the sampling profiler counts collected objects: GC pressure; car
  // models arriving compile shaders in the window).
  maxAllocKBPerFrame: 2400,
  // Every frame over 250 ms loses sim time (slow motion); measured 0–3 in 12 s.
  maxFramesOver250: 12,
  // Crash effects (render/fx, "low" on this profile): CPU per frame incl. emitters and upload
  // prep; measured 0.23–0.44 ms at ~2 fps (docs/integration/crash-fx.md). One draw call per
  // effect kind.
  maxFxMsPerFrame: 1.0,
  maxFxDrawCalls: 5,
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
  if (s.fx) {
    const fxMs = p.sections.fx ?? 0;
    check(
      fxMs <= LIMITS.maxFxMsPerFrame && s.fx.drawCalls <= LIMITS.maxFxDrawCalls,
      `${tag}: crash effects [${s.fx.quality}] ${fxMs.toFixed(3)} ms/frame (≤ ${LIMITS.maxFxMsPerFrame}), ${s.fx.drawCalls} draw calls (≤ ${LIMITS.maxFxDrawCalls}); ` +
        `${s.fx.sparks} sparks, ${s.fx.smoke} puffs, ${s.fx.shards} shards, ${s.fx.chips} chips, ${s.fx.skidSegments} skid segments this session`,
    );
  }
  check(s.programChurn === 0, `${tag}: no shader program re-resolve in steady frames (${s.programChurn} material version bumps over 3 renders${s.churned?.length ? ": " + s.churned.join(", ") : ""})`);
}

console.log(failures === 0 ? "\nperf check passed" : `\n${failures} perf check(s) FAILED`);
process.exit(failures === 0 ? 0 : 1);
