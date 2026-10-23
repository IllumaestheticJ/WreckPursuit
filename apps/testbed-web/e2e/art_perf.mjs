// Art-style perf A/B in the night night run (machine S). Usage (after `npm run wasm`):
// node e2e/art_perf.mjs   Env: CHROMIUM, VIEWPORT=WxH (default 1280x720), SECS (default 8).
// Loads the page fresh in each style (?style=greybox, ?style=lowpoly: the canvas antialiasing
// depends on the style the page starts in), drives the chase and reports fps / frame ms, then,
// in low-poly dark, the post stack alone: the same frame rendered plainly vs through post.ts,
// GPU-synced with readPixels, with the post target's 4× MSAA and without (?msaa=0). SwiftShader is a software GPU: compare numbers, don't read them
// as real frame times. Prints PERF lines; exits 1 on page errors.
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = 5195;
const URL = `http://localhost:${PORT}/`;
const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; ; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
  if (i > 100) throw new Error("vite did not start");
  await new Promise((r) => setTimeout(r, 200));
}
const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const [vw, vh] = (process.env.VIEWPORT ?? "1280x720").split("x").map(Number);
const secs = Number(process.env.SECS ?? 8);
const errors = [];

async function run(style) {
  const page = await (await browser.newContext({ viewport: { width: vw, height: vh } })).newPage();
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(`${URL}?style=${style}`);
  await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
  await page.evaluate(() => {
    document.getElementById("career").checked = false;
    window.__wp.startNightRun("debug_sedan", "Arcade");
  });
  await page.waitForFunction(() => (window.__wp.scene.cities.get("vesper_v2")?.children.length ?? 0) > 0, null, { timeout: 90000 });
  await page.keyboard.down("KeyW");
  await page.waitForTimeout(4000);
  const r = await page.evaluate(async (ms) => {
    const w = window.__wp;
    let frames = 0;
    const t0 = performance.now();
    await new Promise((res) => {
      const tick = () => (++frames, performance.now() - t0 < ms ? requestAnimationFrame(tick) : res());
      requestAnimationFrame(tick);
    });
    const wall = performance.now() - t0;
    return { fps: (frames * 1000) / wall, frameMs: wall / frames, renderCpuMs: w.app.perf.renderMs, calls: w.scene.renderer.info.render.calls };
  }, secs * 1000);
  let post = null;
  if (style.startsWith("lowpoly")) {
    post = await page.evaluate(() => {
      const w = window.__wp;
      w.app.paused = true;
      const r = w.scene.renderer;
      const gl = r.getContext();
      const px = new Uint8Array(4);
      const time = (fn) => {
        fn();
        gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
        const t0 = performance.now();
        for (let i = 0; i < 10; i++) {
          fn();
          gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
        }
        return (performance.now() - t0) / 10;
      };
      const plain = time(() => r.render(w.scene.scene, w.rig.camera));
      const styled = time(() => w.scene.render(w.rig.camera));
      w.style.bloom = false;
      const noBloom = time(() => w.scene.render(w.rig.camera));
      w.style.bloom = true;
      w.app.paused = false;
      return { plain, styled, noBloom, w: gl.drawingBufferWidth, h: gl.drawingBufferHeight };
    });
  }
  await page.context().close();
  return { style, ...r, post };
}

const grey = await run("greybox");
const low = await run("lowpoly");
// Same without the post target's 4× MSAA (SwiftShader multisamples in software, so MSAA
// dominates there; this run isolates the bloom / tonemap passes).
const low0 = await run("lowpoly&msaa=0");
for (const x of [grey, low, low0])
  console.log(`PERF night chase ${x.style} (swiftshader ${vw}×${vh}): ${x.fps.toFixed(2)} fps, ${x.frameMs.toFixed(0)} ms/frame, render CPU ${x.renderCpuMs.toFixed(1)} ms, ${x.calls} draw calls (last pass)`);
for (const [name, p] of [["4× MSAA", low.post], ["no MSAA", low0.post]])
console.log(
  `PERF post stack, ${name} (swiftshader ${p.w}×${p.h}, GPU-synced): scene ${p.plain.toFixed(1)} ms plain → ${p.styled.toFixed(1)} ms through post.ts ` +
    `(+${(p.styled - p.plain).toFixed(1)} ms = ${((100 * (p.styled - p.plain)) / p.plain).toFixed(0)} %; without bloom ${p.noBloom.toFixed(1)} ms)`,
);
await browser.close();
if (errors.length) {
  console.error("page errors:", errors.slice(0, 5));
  process.exit(1);
}
process.exit(0);
