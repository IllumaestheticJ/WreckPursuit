// Does every visible mod show on every car? (headless Chromium). For each car model, renders the
// stock car from a few angles, then each visible mod of data/cosmetics/visuals.json alone (sim
// build parts + cosmetic look, as a career car gets them) and counts the pixels that changed.
// A mod that changes nothing from any angle is reported as missing on that car.
// Usage: node e2e/mod_coverage.mjs [car ...] (needs `npm run wasm` first).
//   MODS=a,b only these mods; SHOTS=dir writes the views of each missing mod (and the stock car);
//   OUT=file.json writes the full table.
import { spawn } from "node:child_process";
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { inflateSync } from "node:zlib";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const repo = join(root, "../..");
const PORT = Number(process.env.PORT ?? 5209);
const URL = `http://localhost:${PORT}/`;
const shots = process.env.SHOTS;
if (shots) mkdirSync(shots, { recursive: true });
const visuals = JSON.parse(readFileSync(join(repo, "data/cosmetics/visuals.json"), "utf8")).mods;
let mods = Object.keys(visuals).filter((id) => Array.isArray(visuals[id]));
if (process.env.MODS) mods = process.env.MODS.split(",");
// Mod defs: prerequisites (a mod is shown on top of what it requires, compared with just those)
// and class limits (a mod the car's class cannot take is skipped).
const defs = {};
for (const f of readdirSync(join(repo, "data/mods")).filter((f) => f.endsWith(".json"))) {
  const j = JSON.parse(readFileSync(join(repo, "data/mods", f), "utf8"));
  for (const m of Array.isArray(j) ? j : j.mods) defs[m.id] = m;
}
const needs = (id) => (defs[id]?.requires ?? []).flatMap((r) => [...needs(r), r]);
const classOf = (car) => JSON.parse(readFileSync(join(repo, "data/cars", `${car}.json`), "utf8")).class;
let cars = process.argv.slice(2);
if (!cars.length)
  cars = readdirSync(join(repo, "data/panels/meshes"))
    .filter((f) => f.endsWith(".json") && !/^(debug|parity)_/.test(f))
    .map((f) => f.slice(0, -5));

/** Decode an 8-bit RGB / RGBA PNG (what Chromium screenshots are) to RGBA pixels. */
function decodePng(buf) {
  let p = 8;
  let w = 0;
  let h = 0;
  let type = 0;
  const idat = [];
  while (p < buf.length) {
    const len = buf.readUInt32BE(p);
    const kind = buf.toString("ascii", p + 4, p + 8);
    const data = buf.subarray(p + 8, p + 8 + len);
    if (kind === "IHDR") {
      w = data.readUInt32BE(0);
      h = data.readUInt32BE(4);
      type = data[9];
    } else if (kind === "IDAT") idat.push(data);
    p += 12 + len;
  }
  const bpp = type === 6 ? 4 : 3;
  const raw = inflateSync(Buffer.concat(idat));
  const stride = w * bpp;
  const cur = Buffer.alloc(stride);
  const prev = Buffer.alloc(stride);
  const out = new Uint8Array(w * h * 4);
  for (let y = 0; y < h; y++) {
    const f = raw[y * (stride + 1)];
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    for (let i = 0; i < stride; i++) {
      const a = i >= bpp ? cur[i - bpp] : 0;
      const b = prev[i];
      const c = i >= bpp ? prev[i - bpp] : 0;
      let v = line[i];
      if (f === 1) v += a;
      else if (f === 2) v += b;
      else if (f === 3) v += (a + b) >> 1;
      else if (f === 4) {
        const pp = a + b - c;
        const pa = Math.abs(pp - a);
        const pb = Math.abs(pp - b);
        const pc = Math.abs(pp - c);
        v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      }
      cur[i] = v & 255;
    }
    for (let x = 0; x < w; x++) for (let k = 0; k < 3; k++) out[(y * w + x) * 4 + k] = cur[x * bpp + k];
    cur.copy(prev);
  }
  return out;
}

/** Pixels that differ by more than a little between two decoded shots. */
function diff(a, b) {
  let n = 0;
  for (let i = 0; i < a.length; i += 4) if (Math.abs(a[i] - b[i]) + Math.abs(a[i + 1] - b[i + 1]) + Math.abs(a[i + 2] - b[i + 2]) > 24) n++;
  return n;
}

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
const page = await (await browser.newContext({ viewport: { width: 560, height: 350 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
await page.goto(`${URL}?gfx=high`);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
await page.evaluate(() => {
  for (const id of ["tireForces", "slipColour", "contacts", "cgVelocity"]) window.__wp.toggleLayer(id);
});

/** Free drive in `car` with `mods` (sim build), paused, panels ready, the look on. */
let tSetup = 0;
let tShoot = 0;
async function setup(car, m) {
  const t = Date.now();
  await setupInner(car, m);
  tSetup += Date.now() - t;
}
async function setupInner(car, m) {
  await page.evaluate(
    ([c, m]) => {
      const w = window.__wp;
      w.startFreeDrive(c, undefined, false, undefined, m);
      w.app.paused = true;
      document.body.classList.add("wp-workshop");
    },
    [car, m],
  );
  await page.waitForFunction(() => window.__wp.app.views.get(window.__wp.app.player)?.panels.ready, null, { timeout: 30000 });
  // One paint for every shot (cars otherwise spawn in a random factory colour).
  await page.evaluate(() => window.__wp.app.views.get(window.__wp.app.player).setPaint(0x8a8f96));
  if (m.length) {
    await page.evaluate((m) => window.__wp.app.views.get(window.__wp.app.player).setLook(m, true), m);
    // The look may be empty (everything drawn by the build): give it a moment either way.
    await page.waitForFunction(() => window.__wp.app.views.get(window.__wp.app.player).look !== null, null, { timeout: 1000 }).catch(() => {});
  }
  await page.waitForTimeout(400);
}

// Car-space eyes (scaled by the car's length), looking at the car's middle.
// Views: eye and target in car space; `k` = 0.95 × car length; `fl` = front-left wheel centre.
// The last two take the body shell off first (engine bay, cabin, trunk).
const VIEWS = {
  front: "eye: [-0.85 * k, 1.05 * k, 0.42 * k + 0.4], at: [0, 0, 0.6]",
  rear: "eye: [0.85 * k, -1.05 * k, 0.42 * k + 0.4], at: [0, 0, 0.6]",
  side: "eye: [-1.2 * k, 0.05 * k, 0.22 * k + 0.4], at: [0, 0, 0.6]",
  top: "eye: [-0.25 * k, -0.35 * k, 1.2 * k + 0.4], at: [0, 0, 0.6]",
  low: "eye: [0.55 * k, -0.95 * k, 0.12], at: [0, -0.3 * k, 0.35]",
  wheel: "eye: [fl[0] - 1.5, fl[1] + 0.25, fl[2] + 0.15], at: fl",
  open_top: "open: true, eye: [-0.2 * k, 0.1 * k, 1.1 * k + 0.4], at: [0, 0, 0.5]",
  open_front: "open: true, eye: [-0.55 * k, 0.9 * k, 0.75 * k + 0.4], at: [0, 0.2 * k, 0.5]",
};
// VIEWS=front,rear: only these views (look sheets).
if (process.env.VIEWS) for (const v of Object.keys(VIEWS)) if (!process.env.VIEWS.split(",").includes(v)) delete VIEWS[v];
/** Shots of every view, or (with `base`) only until one view clearly differs from `base`. */
async function shoot(car, base = null, enough = Infinity) {
  const t = Date.now();
  const out = await shootInner(car, base, enough);
  tShoot += Date.now() - t;
  return out;
}
async function shootInner(car, base, enough) {
  const out = {};
  for (const [name, view] of Object.entries(VIEWS)) {
    await page.evaluate(
      ([src]) => {
        const w = window.__wp;
        const v = w.app.views.get(w.app.player);
        const V = v.root.position.constructor;
        const k = (v.info.body_size_m?.[1] ?? 4.5) * 0.95;
        const fl = v.info.wheels.find((x) => x.position_m[0] < 0 && x.position_m[1] > 0)?.position_m ?? [-0.8, 1.3, 0.3];
        const view = new Function("k", "fl", `return { ${src} };`)(k, fl);
        // Body shell off: what is left is the engine bay, the cabin and the mod pieces.
        if (view.open) v.panels.hideParts((d) => !["interior", "engine", "suspension", "mod"].includes(d.kind));
        // Only the car: the sky, clouds and ground change from run to run.
        const sc = w.scene.scene;
        const mine = new Set();
        v.root.traverse((o) => mine.add(o));
        sc.traverse((o) => {
          if ((o.isMesh || o.isLine || o.isPoints || o.isSprite) && !mine.has(o)) o.visible = false;
        });
        sc.fog = null;
        sc.background = new v.panels.paint.color.constructor(0x9aa4ad);
        sc.environment = null;
        v.root.updateWorldMatrix(true, false);
        const toSim = (p) => {
          const q = v.root.localToWorld(new V(p[0], p[1], p[2]));
          return [q.x, -q.z, q.y];
        };
        w.rig.camera.fov = 60;
        w.rig.camera.updateProjectionMatrix();
        w.rig.lookFrom(toSim(view.eye), toSim(view.at));
      },
      [view],
    );
    await page.waitForTimeout(120);
    const png = await page.screenshot();
    out[name] = { png, px: decodePng(png) };
    if (base && diff(base[name].px, out[name].px) > enough) break;
  }
  return out;
}

const table = {};
const t0 = Date.now();
for (const car of cars) {
  await setup(car, []);
  const stock = await shoot(car);
  if (shots) for (const [v, s] of Object.entries(stock)) writeFileSync(join(shots, `${car}__stock_${v}.png`), s.png);
  // Noise floor: the same stock car shot twice.
  await setup(car, []);
  const again = await shoot(car);
  const noise = Math.max(...Object.keys(VIEWS).map((v) => diff(stock[v].px, again[v].px)));
  const row = (table[car] = { noise, missing: [], skipped: [], counts: {} });
  const cls = classOf(car);
  const bases = new Map([["", stock]]);
  for (const mod of mods) {
    const wl = defs[mod]?.class_whitelist ?? [];
    if (wl.length && !wl.includes(cls)) {
      row.skipped.push(mod);
      continue;
    }
    const pre = [...new Set(needs(mod))];
    const key = pre.join(",");
    if (!bases.has(key)) {
      await setup(car, pre);
      bases.set(key, await shoot(car));
    }
    const base = bases.get(key);
    const before = errors.length;
    await setup(car, [...pre, mod]);
    const s = await shoot(car, base, process.env.SHOTS_ALL ? Infinity : noise + 300);
    const counts = Object.fromEntries(Object.keys(s).map((v) => [v, diff(base[v].px, s[v].px)]));
    const best = Math.max(...Object.values(counts));
    row.counts[mod] = best;
    const miss = best <= noise + 40 || errors.length > before;
    if (miss) row.missing.push(mod);
    if (shots && (miss || process.env.SHOTS_ALL)) { for (const [v, x] of Object.entries(s)) writeFileSync(join(shots, `${car}__${mod}_${v}.png`), x.png);
    }
  }
  console.log(`${car}: ${row.missing.length} of ${mods.length} mods show nothing (noise ${noise} px, ${row.skipped.length} not for ${cls}) ${row.missing.join(" ")}  [${((Date.now() - t0) / 1000).toFixed(0)} s; setup ${(tSetup / 1000).toFixed(0)} s, shots ${(tShoot / 1000).toFixed(0)} s]`);
}
if (process.env.OUT) writeFileSync(process.env.OUT, JSON.stringify(table, null, 1));
const real = errors.filter((e) => !/favicon|AudioContext/.test(e));
if (real.length) console.log(`page errors: ${real.slice(0, 5).join(" | ")}`);
await browser.close();
const total = Object.values(table).reduce((n, r) => n + r.missing.length, 0);
console.log(total ? `${total} car/mod pairs show nothing` : "mod coverage: every mod shows on every car");
process.exit(total ? 1 : 0);
