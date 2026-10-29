// A career car is drawn in one colour everywhere (owner report on v0.14.4: "the car is white in
// garage and orange in freeroam"). The street beater (no saved paint: its default colour comes
// from its uid) is shown in the workshop's 3D bay, on a Free Roam run (low-poly dark and
// greybox), through an instant crash replay and back, and again after a respray at the paint
// shop. Each time the car's paint material must be the career view's colour, and the body
// pixels on screen must have that colour's hue.
// Usage: node e2e/paint_match.mjs (needs `npm run wasm` first). SHOTS=dir for screenshots
// (docs/media/paint-match/ holds the committed ones).
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5219;
const URL = `http://localhost:${PORT}/`;
const shots = process.env.SHOTS;
if (shots) mkdirSync(shots, { recursive: true });

const server = spawn("npx", ["vite", "--port", String(PORT), "--strictPort"], { cwd: root, stdio: "pipe" });
process.on("exit", () => server.kill("SIGTERM"));
for (let i = 0; ; i++) {
  try {
    if ((await fetch(URL)).ok) break;
  } catch {}
  if (i > 150) throw new Error("vite did not start");
  await new Promise((r) => setTimeout(r, 200));
}

let failures = 0;
const check = (ok, msg) => {
  console.log(`${ok ? "PASS" : "FAIL"}  ${msg}`);
  if (!ok) failures++;
};

const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1280, height: 800 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));

const hex = (v) => `#${(v ?? 0).toString(16).padStart(6, "0")}`;
const settle = (ms = 1200) => page.waitForTimeout(ms);

/** Hue (degrees) of 0xRRGGBB. */
function hueOf(c) {
  const r = ((c >> 16) & 255) / 255, g = ((c >> 8) & 255) / 255, b = (c & 255) / 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b), d = max - min;
  if (d === 0) return 0;
  const h = max === r ? ((g - b) / d) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  return (h * 60 + 360) % 360;
}
const hueGap = (a, b) => Math.min(Math.abs(a - b), 360 - Math.abs(a - b));

/** The player car's colours: career view paint, the view's paint and its drawn materials. */
const colours = (uid) =>
  page.evaluate((u) => {
    const w = window.__wp;
    const v = w.app.views.get(w.app.player);
    return {
      scenario: w.sim.scenario().name,
      career: w.sim.careerView().cars.find((c) => c.uid === u)?.paint,
      session: v?.info.paint ?? null,
      view: v?.paintHex,
      panels: v?.panels.paint.color.getHex(),
      proxy: v?.proxyPaint.color.getHex(),
    };
  }, uid);

/** The car's on-screen box (pixels), from its body size projected with the active camera. */
const carRect = () =>
  page.evaluate(() => {
    const w = window.__wp;
    const v = w.app.views.get(w.app.player);
    const cam = w.rig.camera;
    cam.updateMatrixWorld();
    v.root.updateMatrixWorld();
    const V = cam.position.constructor;
    const [bw, bl, bh] = v.info.body_size_m;
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (const sx of [-0.5, 0.5]) for (const sy of [-0.5, 0.5]) for (const sz of [0.15, 0.9]) {
      const p = v.root.localToWorld(new V(sx * bw, sy * bl, sz * bh)).project(cam);
      const px = ((p.x + 1) / 2) * innerWidth, py = ((1 - p.y) / 2) * innerHeight;
      x0 = Math.min(x0, px); x1 = Math.max(x1, px); y0 = Math.min(y0, py); y1 = Math.max(y1, py);
    }
    return { x0, y0, x1, y1 };
  });

/**
 * Dominant hue of the saturated pixels in the middle of the car's screen box (the body panels;
 * glass, tyres and shadow are dark or grey and drop out). Null when too few pixels qualify.
 */
async function bodyHue(png) {
  const r = await carRect();
  const b64 = png.toString("base64");
  return page.evaluate(
    async ({ b64, r }) => {
      const img = new Image();
      img.src = `data:image/png;base64,${b64}`;
      await img.decode();
      const c = document.createElement("canvas");
      c.width = img.width;
      c.height = img.height;
      const g = c.getContext("2d");
      g.drawImage(img, 0, 0);
      const k = img.width / innerWidth;
      const cx = (r.x0 + r.x1) / 2, cy = (r.y0 + r.y1) / 2;
      const hw = ((r.x1 - r.x0) / 2) * 0.7, hh = ((r.y1 - r.y0) / 2) * 0.7;
      const x = Math.max(0, Math.round((cx - hw) * k)), y = Math.max(0, Math.round((cy - hh) * k));
      const w = Math.min(img.width - x, Math.round(2 * hw * k)), h = Math.min(img.height - y, Math.round(2 * hh * k));
      if (w < 4 || h < 4) return null;
      const d = g.getImageData(x, y, w, h).data;
      const bins = new Array(36).fill(0);
      let n = 0;
      for (let i = 0; i < d.length; i += 4) {
        const R = d[i] / 255, G = d[i + 1] / 255, B = d[i + 2] / 255;
        const max = Math.max(R, G, B), min = Math.min(R, G, B), s = max - min;
        if (max < 0.12 || s / max < 0.3) continue;
        let hu = max === R ? ((G - B) / s) % 6 : max === G ? (B - R) / s + 2 : (R - G) / s + 4;
        hu = (hu * 60 + 360) % 360;
        bins[Math.floor(hu / 10) % 36]++;
        n++;
      }
      if (n < 30) return null;
      const best = bins.indexOf(Math.max(...bins));
      return { hue: best * 10 + 5, share: bins[best] / n, pixels: n };
    },
    { b64, r },
  );
}

/** Screenshot (saved under SHOTS) and the body hue of the player car in it. */
async function shotHue(name) {
  const png = await page.screenshot(shots ? { path: join(shots, name) } : {});
  return bodyHue(png);
}

/** All of the car's colours are the career colour, and the screen shows its hue. */
async function expectCareerColour(uid, where, name) {
  const c = await colours(uid);
  const want = c.career;
  check(
    c.session === want && c.view === want && c.panels === want && c.proxy === want,
    `${where}: car drawn in ${hex(want)} (session ${hex(c.session)}, view ${hex(c.view)}, panels ${hex(c.panels)}, proxy ${hex(c.proxy)}; ${c.scenario})`,
  );
  const h = await shotHue(name);
  const gap = h ? hueGap(h.hue, hueOf(want)) : 999;
  check(h !== null && gap <= 30, `${where}: body pixels have the paint's hue (${h ? `${h.hue}° over ${h.pixels} px` : "no body pixels"}, want ${hueOf(want).toFixed(0)}°)`);
  return h;
}

await page.goto(URL);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
await page.evaluate(() => window.__wp.setAutoCrashReplay(false));

// The street beater: no saved paint, so its colour is the per-car default (by uid).
const uid = await page.evaluate(() => {
  const s = window.__wp.sim;
  s.careerNew("Arcade", 7);
  return s.careerFloorCar().active_car;
});
const beater = await page.evaluate((u) => window.__wp.sim.careerView().cars.find((c) => c.uid === u), uid);
check(beater.def_id === "junkyard_beater", `career car is the street beater (uid ${uid}, colour ${hex(beater.paint)})`);

const openBay = async () => {
  await page.evaluate((u) => window.__wp.openWorkshop(u), uid);
  await page.waitForFunction(() => {
    const w = window.__wp;
    const v = w.app.views.get(w.app.player);
    return w.workshop.active && v?.panels.ready && w.scene.world.getObjectByName("workshop-room");
  }, null, { timeout: 60000 });
  await settle();
};
const roam = async () => {
  await page.evaluate(() => window.__wp.openGarage());
  await page.evaluate(() => window.__wp.careerApi.startRun("free", { time: "day", weather: "clear" }));
  await page.waitForFunction(() => {
    const w = window.__wp;
    return w.sim.careerRunActive() && w.app.views.get(w.app.player)?.panels?.ready && !w.garage.open;
  }, null, { timeout: 60000 });
  await page.evaluate(() => window.__wp.setCamera("chase"));
  await settle(2500);
};

// ----- garage bay vs Free Roam ----------------------------------------------------------------
await openBay();
const bayHue = await expectCareerColour(uid, "garage bay", "01_garage_bay.png");
await roam();
const roamHue = await expectCareerColour(uid, "Free Roam", "02_free_roam.png");
check(bayHue && roamHue && hueGap(bayHue.hue, roamHue.hue) <= 30, `garage bay and Free Roam show the same body colour (${bayHue?.hue}° vs ${roamHue?.hue}°)`);

// Greybox art style: the panel mesh used to keep the per-entity default colour there.
await page.evaluate(() => window.__wp.setStyle("greybox"));
await settle();
await expectCareerColour(uid, "Free Roam (greybox)", "03_free_roam_greybox.png");
await page.evaluate(() => window.__wp.setStyle("lowpoly"));
await settle();

// ----- instant crash replay: the views are rebuilt for the replay session and back -------------
// A flat career free drive with a parked car ahead and the beater placed at speed into it.
await page.evaluate(() => window.__wp.openGarage());
await page.evaluate(() => window.__wp.startCareerRun("free", false, false));
await page.waitForFunction(() => window.__wp.sim.careerRunActive() && window.__wp.app.views.get(window.__wp.app.player)?.panels?.ready, null, { timeout: 60000 });
await page.evaluate(() => {
  const w = window.__wp;
  const s = w.sim.states();
  const x = s[1], y = s[2];
  w.sim.spawn("junkyard_beater", [x, y + 30, 0], 0);
  w.setAutoCrashReplay(true);
  w.sim.debugPlace(w.app.player, x, y + 5, 0, 22);
});
const replayed = await page
  .waitForFunction(() => window.__wp.crash.phase === "playing", null, { timeout: 30000 })
  .then(() => true)
  .catch(() => false);
check(replayed, "a hard crash plays the instant crash replay");
if (replayed) {
  await settle(600);
  await expectCareerColour(uid, "crash replay", "04_crash_replay.png");
  await page.evaluate(() => window.__wp.crash.skip());
  await page.waitForFunction(() => window.__wp.crash.phase === "idle", null, { timeout: 30000 });
  await settle();
  const back = await colours(uid);
  check(back.session === back.career && back.panels === back.career, `back from the replay: still ${hex(back.career)} (panels ${hex(back.panels)})`);
}
await page.evaluate(() => window.__wp.setAutoCrashReplay(false));

// ----- paint shop: a respray shows in the bay and on the road ----------------------------------
const GREEN = 0x3fb950;
await page.evaluate(() => window.__wp.openGarage());
await page.evaluate(({ u, c }) => window.__wp.sim.careerRespray(u, c), { u: uid, c: GREEN });
await openBay();
const c5 = await expectCareerColour(uid, "garage bay after a respray", "05_garage_bay_resprayed.png");
await roam();
const c6 = await expectCareerColour(uid, "Free Roam after a respray", "06_free_roam_resprayed.png");
const now = await colours(uid);
check(now.career === GREEN, `the respray is saved on the car (${hex(now.career)})`);
check(c5 && c6 && hueGap(c5.hue, c6.hue) <= 30, `resprayed: bay and Free Roam match (${c5?.hue}° vs ${c6?.hue}°)`);

const real = errors.filter((e) => !/favicon|404/.test(e));
check(real.length === 0, `no page errors (${real.slice(0, 3).join(" | ")})`);
await browser.close();
console.log(failures ? `${failures} FAILED` : "all passed");
process.exit(failures ? 1 : 0);
