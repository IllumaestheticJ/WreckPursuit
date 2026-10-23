// Garage workshop screenshots per car (exploration / review aid): opens each car in the workshop,
// optionally removes slots, and writes one shot per camera preset.
// Usage: CARS=hakone_gt,vireo_xs REMOVE=rear_bumper CAMS=overview,under OUT=dir node e2e/workshop_mounts_shots.mjs
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { chromiumPath } from "./chromium.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5211;
const URL = `http://localhost:${PORT}/`;
const out = process.env.OUT ?? "/tmp/ws_shots";
mkdirSync(out, { recursive: true });
const cars = (process.env.CARS ?? "hakone_gt").split(",");
const remove = (process.env.REMOVE ?? "").split(",").filter(Boolean);
const cams = (process.env.CAMS ?? "overview").split(",").filter(Boolean);
const install = (process.env.INSTALL ?? "").split(",").filter(Boolean);
// Extra close-ups: name → [eye, at] in car space (metres), after the camera presets.
const VIEWS = {
  bay: [[-0.9, 2.6, 1.9], [0, 1.2, 0.45]],
  under_rear: [[0.9, -0.5, -1.2], [0, -1.3, 0.2]],
  under_front: [[0.9, 0.4, -1.2], [0, 1.3, 0.2]],
  tail: [[1.4, -4.2, 0.9], [0, -2.2, 0.45]],
  nose: [[1.4, 4.4, 0.9], [0, 2.2, 0.45]],
  cab: [[0.8, -3.6, 2.4], [0, -0.4, 1.4]],
};
const views = (process.env.VIEWS ?? "").split(",").filter(Boolean);

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
  executablePath: chromiumPath(),
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 1200, height: 760 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
const placed = () =>
  page.waitForFunction(() => {
    const w = window.__wp;
    const v = w.app.views.get(w.app.player);
    return w.workshop.active && v?.panels.ready && w.scene.world.getObjectByName("workshop-room");
  }, null, { timeout: 30000 });
await page.goto(URL + "?gfx=high");
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });
for (const car of cars) {
  const { uid, errs } = await page.evaluate(
    ({ car, install }) => {
      const s = window.__wp.sim;
      s.careerNew("Arcade");
      const save = JSON.parse(s.careerSave());
      const bump = (o) => {
        for (const k of Object.keys(o)) {
          if (k === "cash") o[k] = 10_000_000;
          else if (o[k] && typeof o[k] === "object") bump(o[k]);
        }
      };
      bump(save);
      s.careerLoad(JSON.stringify(save));
      const v = s.careerBuy(car, "Worn");
      const uid = v.cars[v.cars.length - 1].uid;
      const errs = [];
      for (const m of install) {
        try {
          s.careerInstall(uid, m);
        } catch (e) {
          errs.push(`install ${m}: ${String(e).split("\n")[0]}`);
        }
      }
      return { uid, errs };
    },
    { car, install },
  );
  for (const e of errs) console.log(`${car}: ${e}`);
  for (const slot of remove) {
    try {
      await page.evaluate(({ uid, slot }) => window.__wp.sim.careerRemovePart(uid, slot), { uid, slot });
    } catch (e) {
      console.log(`${car}: cannot remove ${slot}: ${String(e).split("\n")[0]}`);
    }
  }
  await page.evaluate((u) => window.__wp.openWorkshop(u), uid);
  await placed();
  await page.waitForTimeout(1200);
  for (const cam of cams) {
    await page.evaluate((c) => window.__wp.workshop.camera(c), cam);
    await page.waitForTimeout(cam === "under" ? 2600 : 900);
    await page.screenshot({ path: join(out, `${car}_${cam}.png`) });
  }
  for (const v of views) {
    const lift = v.startsWith("under");
    await page.evaluate((c) => window.__wp.workshop.camera(c), lift ? "under" : "overview");
    await page.waitForTimeout(lift ? 2600 : 600);
    await page.evaluate(([e, a]) => window.__wp.workshop.lookAt(e, a), VIEWS[v]);
    await page.waitForTimeout(700);
    await page.screenshot({ path: join(out, `${car}_${v}.png`) });
  }
  const hw = await page.evaluate(() => window.__wp.workshop.hardwareSlots());
  console.log("shot", car, "hardware:", hw.join(" "));
  console.log("shot", car, "undrawn:", (await page.evaluate(() => window.__wp.workshop.undrawnSlots())).join(" "));
}
const real = errors.filter((e) => !/favicon|AudioContext/.test(e));
if (real.length) console.log("errors:", real.slice(0, 5).join(" | "));
await browser.close();
process.exit(0);
