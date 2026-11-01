// Wider tyres read at a glance (headless Chromium): the wide-tyre stance (render/cosmetics/
// stance.ts) fits every car in the roster without leaving the arch, is drawn the same from a
// build (scenario mods: player, AI, lot cars) and from a cosmetic look (career car, workshop
// preview), comes off again when the preview drops the part, and is cosmetic only (the sim's
// wheels are the same with and without the look). Before / after screenshots: rear
// three-quarter chase view (greybox and low-poly dark) and the garage workshop, stock vs. each
// width grade, on three cars.
// Usage: node e2e/wide_tyres.mjs (needs `npm run wasm` first). SHOTS=dir (default
// docs/media/wide-tyres; SHOTS=0 for none).
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const PORT = 5231;
const URL = `http://localhost:${PORT}/`;
const shots = process.env.SHOTS === "0" ? null : (process.env.SHOTS ?? join(root, "../../docs/media/wide-tyres"));
if (shots) mkdirSync(shots, { recursive: true });

/** Width grades the mod data has (data/mods/tires.json, line "width"): grade → mods. */
const GRADES = { 1: ["tires_wide"] };
/** Cars shot: a mid-size coupe, the narrowest arches in the roster, a pickup. */
const CARS = process.env.CARS ? process.env.CARS.split(",") : ["kestrel_ts", "junkyard_beater", "hauler_1500"];

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
const page = await (await browser.newContext({ viewport: { width: 1100, height: 680 } })).newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
const shot = async (name) => shots && page.screenshot({ path: join(shots, name) });

await page.goto(`${URL}?gfx=high`);
await page.waitForFunction(() => window.__wp?.ready === true, null, { timeout: 60000 });

// ---- Every car in the roster: the stance fits under its arches -------------------------------
const roster = await page.evaluate(async () => {
  const st = await import("/src/render/cosmetics/stance.ts");
  const panels = await import("/src/render/panels.ts");
  const ids = window.__wp.sim.carIds ? window.__wp.sim.carIds() : null;
  const list = ids ?? (await (await fetch("data/cars/ROSTER.md")).text()).match(/\b[a-z]+_[a-z0-9]+\b/g);
  const out = [];
  for (const id of [...new Set(list)]) {
    const car = await fetch(`data/cars/${id}.json`).then((r) => (r.ok ? r.json() : null)).catch(() => null);
    const mesh = car && (await panels.loadCarMesh(id));
    if (!car?.wheels || !mesh) continue;
    for (const g of [1, 2, 3]) {
      car.wheels.slice(0, 4).forEach((w, i) => {
        let tyreHalf = 0;
        for (const p of mesh.parts) if (p.wheel === i && p.kind === "tire") for (let k = 0; k < p.positions_mm.length; k += 3) tyreHalf = Math.max(tyreHalf, Math.abs(p.positions_mm[k]) / 1000);
        const arch = st.traceArch(mesh, w.position_m, w.radius_m);
        const archX = st.archTopX(arch);
        const fit = { hubX: Math.abs(w.position_m[0]), tyreHalf, archX, front: w.position_m[1] > 0 };
        const c = st.solveCorner(fit, g);
        const outer = fit.hubX + c.push + tyreHalf * c.scale;
        const inner = fit.hubX + c.push - tyreHalf * c.scale;
        out.push({ id, g, i, archX, scale: c.scale, push: c.push, lip: c.lip, outer, inner, stockInner: fit.hubX - tyreHalf, stockOuter: fit.hubX + tyreHalf, samples: arch.length });
      });
    }
  }
  return out;
});
const cars = [...new Set(roster.map((r) => r.id))];
check(cars.length >= 30, `roster: ${cars.length} cars measured`);
const noArch = roster.filter((r) => r.archX === null);
check(noArch.length === 0, `every arch is traced (${[...new Set(noArch.map((r) => r.id))].join(", ") || "all found"})`);
// The outside of the lip (a stock tyre already out of the arch edge counts as flush).
const lipX = (r) => Math.max(r.archX, r.stockOuter + 0.008) + r.lip;
const out = roster.filter((r) => r.archX !== null && r.outer > lipX(r) - 0.008 + 1e-6);
check(out.length === 0, `no tyre leaves its arch at any grade (${out.slice(0, 3).map((r) => `${r.id} g${r.g} w${r.i}`).join(", ") || "min room under the lip " + (Math.min(...roster.filter((r) => r.archX !== null).map((r) => lipX(r) - r.outer)) * 100).toFixed(1) + " cm"})`);
const inward = roster.filter((r) => r.g === 1 && r.inner < r.stockInner - 0.03);
check(inward.length === 0, `inner faces move in by at most 3 cm (suspension side) (${inward.slice(0, 3).map((r) => r.id).join(", ") || "ok"})`);
for (const r of [...out, ...inward, ...roster.filter((x) => x.scale < 1.11)].slice(0, 8)) console.log("      " + JSON.stringify(r));
for (const g of [1, 2, 3]) {
  const rows = roster.filter((r) => r.g === g);
  const min = Math.min(...rows.map((r) => r.scale));
  const push = rows.reduce((s, r) => s + r.push, 0) / rows.length;
  check(min >= 1 + (g === 1 ? 0.11 : 0.2), `grade ${g}: tread ≥ +${Math.round((min - 1) * 100)} % on every car (mean push ${(push * 100).toFixed(1)} cm)`);
}
for (const id of ["junkyard_beater", "furrow_24", "kestrel_ts"]) {
  const r = roster.filter((x) => x.id === id && x.g === 1);
  console.log(`      ${id} g1: ` + r.map((x) => `w${x.i} ×${x.scale.toFixed(2)} push ${(x.push * 100).toFixed(1)} cm, ${((lipX(x) - x.outer) * 100).toFixed(1)} cm inside a ${(x.lip * 100).toFixed(1)} cm lip`).join(" | "));
}

if (process.env.ROSTER_ONLY) {
  await browser.close();
  process.exit(failures ? 1 : 0);
}

// ---- Chase view: stock vs. each grade, from the build (scenario mods) ------------------------
/** Free drive in `car` with `mods` installed (the build path), paused, mesh ready. */
async function drive(car, mods) {
  await page.evaluate(([c, m]) => window.__wp.startFreeDrive(c, "Arcade", false, undefined, m), [car, mods]);
  await page.waitForFunction(() => window.__wp.app.views.get(window.__wp.app.player)?.panels.ready, null, { timeout: 30000 });
  await page.waitForTimeout(600);
  await page.evaluate(() => {
    window.__wp.app.paused = true;
    document.getElementById("overlay").style.display = "none";
    for (const id of ["cgVelocity", "tireForces", "slipColour", "telemetry", "components", "chase", "contacts"]) {
      if (window.__wp.layersOn(id)) window.__wp.toggleLayer(id);
    }
    // The origin axis hint stands right by the spawn point.
    const sc = window.__wp.scene;
    (sc.scene ?? sc).traverse((o) => o.type === "AxesHelper" && (o.visible = false));
  });
  await page.waitForTimeout(200);
}
/** Camera from car-space eye to car-space target. */
async function cam(eye, at) {
  await page.evaluate(([e, a]) => {
    const w = window.__wp;
    const v = w.app.views.get(w.app.player);
    const V = v.root.position.constructor;
    const toSim = (p) => {
      const q = v.root.localToWorld(new V(p[0], p[1], p[2]));
      return [q.x, -q.z, q.y];
    };
    w.rig.lookFrom(toSim(e), toSim(a));
  }, [eye, at]);
  await page.waitForTimeout(500);
}
/** The player view's stance and its sim wheels. */
const state = () =>
  page.evaluate(() => {
    const w = window.__wp;
    const v = w.app.views.get(w.app.player);
    return {
      grade: v.stanceGrade,
      corner: v.stance ? v.stance.corner.map((c) => ({ ...c })) : null,
      lips: v.stance ? v.stance.root.children.length : 0,
      spin: v.wheels.map((x) => [x.spin.scale.x, x.spin.position.x]),
      wheels: w.sim.info(w.app.player).wheels.map((x) => [x.width_m, ...x.position_m]),
    };
  });
const REAR34 = [[1.75, -4.3, 1.45], [0.05, -0.15, 0.45]];

for (const style of ["greybox", "lowpoly"]) {
  await page.evaluate((s) => window.__wp.setStyle(s), style);
  for (const car of style === "greybox" ? CARS : CARS.slice(0, 1)) {
    await drive(car, []);
    const stock = await state();
    check(stock.grade === 0 && stock.spin.every(([s, p]) => s === 1 && p === 0), `${style} ${car} stock: wheels untouched`);
    await cam(...REAR34);
    await shot(`chase_${car}_${style}_stock.png`);
    for (const [g, mods] of Object.entries(GRADES)) {
      await drive(car, mods);
      const s = await state();
      check(s.grade === Number(g) && s.lips === 4, `${style} ${car} grade ${g}: stance drawn (${s.lips} arch lips)`);
      const c = s.corner ?? [];
      check(c.every((x) => x.scale >= 1.11 && x.dish > 0), `${style} ${car} grade ${g}: tread ×${c.map((x) => x.scale.toFixed(2)).join("/")}, push ${c.map((x) => (x.push * 100).toFixed(1)).join("/")} cm`);
      check(s.wheels.every((w, i) => Math.abs(w[0] - stock.wheels[i][0] - 0.02 * Number(g)) < 1e-4 && w.slice(1).every((v, k) => v === stock.wheels[i][k + 1])), `${style} ${car} grade ${g}: physics wheels only gain the mod's width (+${2 * Number(g)} cm), same positions`);
      await cam(...REAR34);
      await shot(`chase_${car}_${style}_grade${g}.png`);
    }
  }
}
await page.evaluate(() => window.__wp.setStyle("greybox"));

// ---- Full steering lock and full bump on the front axle (the tightest arches) ----------------
/** Hold the front wheels at full left lock and full bump (render only: the sim is paused). */
const lockFront = () =>
  page.evaluate(() => {
    const w = window.__wp;
    const v = w.app.views.get(w.app.player);
    const update = v.update.bind(v);
    v.update = (...args) => {
      update(...args);
      v.info.wheels.forEach((wi, i) => {
        if (wi.position_m[1] <= 0) return;
        v.wheels[i].pivot.rotation.z = 0.58;
        v.wheels[i].pivot.position.z = wi.position_m[2] + wi.travel_m - (wi.static_compression_m ?? wi.travel_m * 0.5);
      });
    };
  });
const FRONT34 = [[-2.1, 3.0, 0.95], [-0.55, 1.15, 0.4]];
for (const car of ["junkyard_beater", "furrow_24"]) {
  for (const [name, mods] of [["stock", []], ...Object.entries(GRADES).map(([g, m]) => [`grade${g}`, m])]) {
    await drive(car, mods);
    await lockFront();
    await cam(...FRONT34);
    await shot(`lock_${car}_${name}.png`);
  }
}

// ---- Garage workshop: stock, then the part previewed, then fitted ----------------------------
const placed = () =>
  page.waitForFunction(() => {
    const w = window.__wp;
    const v = w.app.views.get(w.app.player);
    return w.workshop.active && v?.panels.ready && w.scene.world.getObjectByName("workshop-room");
  }, null, { timeout: 30000 });
const career = await page.evaluate(() => {
  const s = window.__wp.sim;
  const v = s.careerNewWithCash("Arcade", 500000, 1);
  return { dealer: v.dealer.map((d) => d.def_id) };
});
console.log(`      dealer: ${career.dealer.join(", ")}`);
for (const want of CARS.slice(0, 2)) {
  // The car if the dealer can sell it, else another from the lot.
  const bought = await page.evaluate(([c, dealer]) => {
    const s = window.__wp.sim;
    for (const id of [c, ...dealer]) {
      try {
        const v = s.careerBuy(id, "Worn");
        return { car: id, uid: v.cars[v.cars.length - 1].uid };
      } catch {}
    }
    return null;
  }, [want, career.dealer]);
  check(!!bought, `garage: bought a ${bought?.car ?? want}`);
  if (!bought) continue;
  const { car, uid } = bought;
  await page.evaluate((u) => window.__wp.openWorkshop(u), uid);
  await placed();
  await page.waitForTimeout(1200);
  /** Garage shots: the whole car, and the wheels close up. */
  const garageShots = async (name) => {
    for (const preset of ["overview", "wheels"]) {
      await page.evaluate((p) => window.__wp.workshop.camera(p), preset);
      await page.waitForTimeout(1100);
      await shot(`garage_${car}_${preset}_${name}.png`);
    }
  };
  const real = await page.evaluate((u) => window.__wp.sim.careerView().cars.find((c) => c.uid === u).mods.map((m) => m.id), uid);
  let s = await state();
  check(s.grade === 0, `garage ${car}: stock`);
  await garageShots("stock");
  for (const [g, mods] of Object.entries(GRADES)) {
    // Previewed (what the paint shop / mod picker does), not bought.
    await page.evaluate((m) => window.__wp.app.views.get(window.__wp.app.player).setLook(m, true), [...real, ...mods]);
    await page.waitForFunction((gg) => window.__wp.app.views.get(window.__wp.app.player).stanceGrade === gg, Number(g), { timeout: 10000 });
    await page.waitForTimeout(600);
    s = await state();
    check(s.lips === 4 && (s.corner ?? []).every((x) => x.scale > 1.1), `garage ${car}: grade ${g} previewed`);
    await garageShots(`grade${g}`);
    // Preview dropped: back to stock.
    await page.evaluate((m) => window.__wp.app.views.get(window.__wp.app.player).setLook(m, true), real);
    await page.waitForFunction(() => window.__wp.app.views.get(window.__wp.app.player).stanceGrade === 0, null, { timeout: 10000 });
    s = await state();
    check(s.lips === 0 && s.spin.every(([sc, p]) => sc === 1 && p === 0), `garage ${car}: preview off, wheels stock again`);
  }
  // Fitted for real (installed mod: the build path draws it after the rebuild).
  const ok = await page.evaluate((u) => {
    try {
      window.__wp.sim.careerInstall(u, "tires_wide");
      return true;
    } catch (e) {
      return String(e);
    }
  }, uid);
  if (ok === true) {
    await page.evaluate((u) => window.__wp.openWorkshop(u), uid);
    await placed();
    await page.waitForFunction(() => window.__wp.app.views.get(window.__wp.app.player).stanceGrade === 1, null, { timeout: 10000 }).catch(() => {});
    s = await state();
    check(s.grade === 1 && s.lips === 4, `garage ${car}: installed wider tyres show (grade ${s.grade})`);
  } else console.log(`      (${car}: install skipped: ${ok})`);
}

const real = errors.filter((e) => !/favicon|AudioContext/.test(e));
check(real.length === 0, `no page errors (${real.slice(0, 3).join(" | ")})`);
await browser.close();
console.log(failures ? `${failures} FAILED` : "wide tyres e2e: all passed");
process.exit(failures ? 1 : 0);
