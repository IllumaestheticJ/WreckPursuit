// Smooth-shaded car bodies (car-shapes phase 3b, render/crease.ts): before / after shots in the
// real renderer. Usage (after `npm run wasm`): node e2e/smooth_shots.mjs <outDir> [car ...]
// Writes <outDir>/{before,after,lowpoly}/<car>_<view>.png: `before` = ?flatcars=1 (the old flat
// body), `after` = greybox with crease-split normals, `lowpoly` = the low-poly dark style (still
// faceted). Env: CHROMIUM. Exits 1 on page errors.
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const [outArg, ...carArgs] = process.argv.slice(2);
const out = resolve(outArg ?? join(root, "../../docs/media/car-shapes/phase3/smooth"));
const cars = carArgs.length ? carArgs : ["kestrel_ts", "vireo_xs", "ombra_v12", "ironside_440", "hauler_1500", "sentinel_cruiser", "nyx_proto", "tansy_13"];
const PORT = Number(process.env.PORT ?? 5213);
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
const VIEWS = {
  front: "eye: [-0.62 * k, 0.78 * k, 0.30 * k + 0.4], at: [0, 0.05 * k, 0.6]",
  rear: "eye: [0.62 * k, -0.78 * k, 0.30 * k + 0.4], at: [0, -0.05 * k, 0.6]",
  side: "eye: [-1.05 * k, 0.05 * k, 0.15 * k + 0.4], at: [0, 0, 0.6]",
};
const errors = [];
for (const [variant, query] of [
  ["before", "?gfx=high&style=greybox&flatcars=1"],
  ["after", "?gfx=high&style=greybox"],
  ["lowpoly", "?gfx=high&style=lowpoly"],
]) {
  const dir = join(out, variant);
  mkdirSync(dir, { recursive: true });
  const page = await (await browser.newContext({ viewport: { width: 900, height: 560 } })).newPage();
  page.on("pageerror", (e) => errors.push(`${variant}: ${e}`));
  await page.goto(URL + query);
  await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
  await page.evaluate(() => {
    for (const id of ["tireForces", "slipColour", "contacts", "cgVelocity"]) window.__wp.toggleLayer(id);
  });
  for (const car of cars) {
    await page.evaluate((c) => {
      const w = window.__wp;
      w.startFreeDrive(c, undefined, false, undefined, []);
      w.app.paused = true;
      document.body.classList.add("wp-workshop");
    }, car);
    await page.waitForFunction(() => window.__wp.app.views.get(window.__wp.app.player)?.panels.ready, null, { timeout: 30000 });
    await page.evaluate(() => window.__wp.app.views.get(window.__wp.app.player).setPaint(0x9a3a2c));
    await page.waitForTimeout(400);
    for (const [name, src] of Object.entries(VIEWS)) {
      await page.evaluate((src) => {
        const w = window.__wp;
        const v = w.app.views.get(w.app.player);
        const V = v.root.position.constructor;
        const k = (v.info.body_size_m?.[1] ?? 4.5) * 0.95;
        const view = new Function("k", `return { ${src} };`)(k);
        const sc = w.scene.scene;
        const mine = new Set();
        v.root.traverse((o) => mine.add(o));
        sc.traverse((o) => {
          if ((o.isMesh || o.isLine || o.isPoints || o.isSprite) && !mine.has(o)) o.visible = false;
        });
        sc.fog = null;
        sc.background = new v.panels.paint.color.constructor(0x9aa4ad);
        v.root.updateWorldMatrix(true, false);
        const toSim = (p) => {
          const q = v.root.localToWorld(new V(p[0], p[1], p[2]));
          return [q.x, -q.z, q.y];
        };
        w.rig.camera.fov = 40;
        w.rig.camera.updateProjectionMatrix();
        w.rig.lookFrom(toSim(view.eye), toSim(view.at));
      }, src);
      await page.waitForTimeout(150);
      writeFileSync(join(dir, `${car}_${name}.png`), await page.screenshot());
    }
    console.log(`${variant} ${car}`);
  }
  await page.close();
}
await browser.close();
server.kill("SIGTERM");
if (errors.length) {
  console.error(errors.join("\n"));
  process.exit(1);
}
process.exit(0);
