// Static-frame cost of each part of the low-poly dark style (machine S). Pauses one night
// city-chase frame and times GPU-synced renders (render + 1-pixel readPixels) with each part
// toggled live, so every row sees the same view. Usage: node e2e/art_cost.mjs (needs
// `npm run wasm`; own Vite). Env: QUERY=<url params> (e.g. gfx=high), N=<renders per row>.
// Headless SwiftShader: compare rows with each other only.
import { spawn } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const PORT = Number(process.env.PORT ?? 5196);
const N = Number(process.env.N ?? 8);
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
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1280, height: 720 } })).newPage();
await page.goto(`http://localhost:${PORT}/?style=lowpoly${process.env.QUERY ? `&${process.env.QUERY}` : ""}`);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 90000 });
await page.evaluate(() => window.__wp.startCityChase(undefined, "Arcade", false));
await page.keyboard.down("KeyW");
await page.waitForTimeout(8000);
await page.keyboard.up("KeyW");
const rows = await page.evaluate(async (n) => {
  const w = window.__wp;
  const s = w.style;
  const sc = w.scene;
  w.app.paused = true;
  const frame = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  await frame();
  const gl = sc.renderer.getContext();
  const px = new Uint8Array(4);
  const time = () => {
    const ts = [];
    for (let i = 0; i < n; i++) {
      const t0 = performance.now();
      sc.render(w.rig.camera);
      gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
      ts.push(performance.now() - t0);
    }
    ts.sort((a, b) => a - b);
    return ts[Math.floor(n / 2)];
  };
  const find = (pred) => {
    const out = [];
    sc.scene.traverse((o) => pred(o) && out.push(o));
    return out;
  };
  const hide = (objs) => {
    const was = objs.map((o) => o.visible);
    objs.forEach((o) => (o.visible = false));
    return () => objs.forEach((o, i) => (o.visible = was[i]));
  };
  const rows = [];
  const row = (name, undo) => {
    time(); // warm (shader compiles)
    rows.push([name, time()]);
    undo?.();
  };
  row("low-poly dark (all)");
  const plain = sc.renderOverride;
  sc.renderOverride = (c) => sc.renderer.render(sc.scene, c);
  row("− post stack (plain render)", () => (sc.renderOverride = plain));
  s.bloom = false;
  row("− bloom", () => (s.bloom = true));
  row("− street lamps", hide(find((o) => o.name === "style-street-lamps")));
  row("− light pools", hide(find((o) => o.isInstancedMesh && o.renderOrder === 2)));
  row("− sky dome", hide(find((o) => o.renderOrder === -10)));
  const bu = find((o) => o.material?.name === "building").map((o) => sc.renderer.properties.get(o.material).uniforms).find((u) => u?.uStyleOn);
  if (bu) {
    bu.uStyleOn.value = 0;
    row("− building windows", () => (bu.uStyleOn.value = 1));
  }
  row("− buildings", hide(find((o) => o.material?.name === "building")));
  row("− fleet", hide([w.lod.fleet.group]));
  row("− vehicle views", hide([...w.app.views.values()].map((v) => v.root)));
  const fog = sc.scene.fog;
  sc.scene.fog = null;
  row("− fog (same tiles)", () => (sc.scene.fog = fog));
  w.setStyle("greybox");
  await frame();
  sc.scene.fog = null;
  row("greybox (switched)");
  w.setStyle("lowpoly");
  await frame();
  row("low-poly dark (again)");
  return { rows, gfx: sc.quality.label(), calls: sc.renderer.info.render.calls };
}, N);
console.log(`static night chase frame, ${rows.gfx}, median of ${N} synced renders`);
for (const [name, ms] of rows.rows) console.log(`${name.padEnd(30)} ${ms.toFixed(1)} ms`);
await browser.close();
process.exit(0);
