// Cost of the smooth-shaded body (car-shapes phase 3b, render/crease.ts): times
// PanelBody.update (skinning + the crease normals of the paint / livery batches) per car with
// every lattice node moving, with only the hood's nodes moving, and with nothing moving, smooth
// vs ?flatcars=1. Usage (after `npm run wasm`): node e2e/smooth_perf.mjs [car ...]
// Prints PERF lines (ms per update; SwiftShader / headless: compare, don't read as frame times).
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const cars = process.argv.slice(2).length ? process.argv.slice(2) : ["kestrel_ts", "ombra_v12", "hauler_1500", "sentinel_cruiser"];
const PORT = Number(process.env.PORT ?? 5214);
const URL = `http://localhost:${PORT}/`;
const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; ; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
  if (i > 150) throw new Error("vite did not start");
  await new Promise((r) => setTimeout(r, 200));
}
const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1243/chrome-linux64/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
for (const [variant, query] of [
  ["flat", "?flatcars=1"],
  ["smooth", ""],
]) {
  const page = await (await browser.newContext({ viewport: { width: 640, height: 400 } })).newPage();
  await page.goto(URL + query);
  await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
  for (const car of cars) {
    await page.evaluate((c) => {
      const w = window.__wp;
      w.startFreeDrive(c, undefined, false, undefined, []);
      w.app.paused = true;
    }, car);
    await page.waitForFunction(() => window.__wp.app.views.get(window.__wp.app.player)?.panels.ready, null, { timeout: 30000 });
    const r = await page.evaluate(() => {
      const w = window.__wp;
      const panels = w.app.views.get(w.app.player).panels;
      const rest = panels.restNodes;
      const n = rest.length / 3;
      // Hood nodes: the region whose name is "Hood".
      const regions = panels.regions ?? [];
      const hoodIdx = regions.indexOf("Hood");
      const nodeRegions = panels.nodeRegions;
      const run = (mode, iters) => {
        const t0 = performance.now();
        for (let it = 0; it < iters; it++) {
          const nodes = mode === "none" ? rest : rest.slice();
          if (mode !== "none")
            for (let i = 0; i < n; i++) {
              if (mode === "hood" && nodeRegions[i] !== hoodIdx) continue;
              const a = 0.004 * Math.sin(it * 0.7 + i);
              nodes[i * 3] += a;
              nodes[i * 3 + 2] -= a;
            }
          panels.update(nodes, 0);
        }
        return (performance.now() - t0) / iters;
      };
      run("all", 20);
      const verts = panels.batches.reduce((s, b) => s + b.position.count, 0);
      return { all: run("all", 150), hood: run("hood", 150), none: run("none", 150), verts, hoodIdx };
    });
    console.log(`PERF ${variant} ${car} all=${r.all.toFixed(3)}ms hood=${r.hood.toFixed(3)}ms none=${r.none.toFixed(4)}ms batchVerts=${r.verts}`);
  }
  await page.close();
}
await browser.close();
server.kill("SIGTERM");
process.exit(0);
