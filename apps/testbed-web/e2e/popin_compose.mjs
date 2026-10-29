// Compose the docs/media/popin/ images from e2e/popin_shots.mjs raw frames and the line-of-sight
// classification (wp_world tests/popin.rs `classify_browser_popins` → `<label>_los.json`).
//
// Usage: node e2e/popin_compose.mjs <outDir> <label>... (e.g. ../../docs/media/popin before after)
// Env: RAW=<dir> (as for popin_shots.mjs; default <outDir>/raw), CHROMIUM=/path/to/chrome.
//
// Per shot `<label>_<shot>.png`: the 6 chase-camera strip frames (1 s apart) with a ring on every
// traffic car that appeared (red: in plain view, grey: behind cover) or vanished (cyan: in plain
// view, dark: behind cover) inside the camera's view cone in the second before. Per shot
// `<label>_<shot>_zoom.png`: the nearest car that appeared in plain view, the frame before and
// the frame it appeared in (2× crop); without one, the nearest that appeared behind cover.
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";

const here = dirname(fileURLToPath(import.meta.url));
const out = resolve(process.argv[2] ?? join(here, "../../../docs/media/popin"));
const labels = process.argv.slice(3).length ? process.argv.slice(3) : ["before", "after"];
const raw = resolve(process.env.RAW ?? join(out, "raw"));

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium-1243/chrome-linux64/chrome" });
const page = await browser.newPage({ viewport: { width: 1340, height: 400 } });
const img = (dir, step) => `data:image/png;base64,${readFileSync(join(dir, `f${step}.png`)).toString("base64")}`;

for (const label of labels) {
  const summary = JSON.parse(readFileSync(join(out, `${label}_summary.json`), "utf8"));
  const losFile = join(out, `${label}_los.json`);
  if (!existsSync(losFile)) throw new Error(`${losFile} missing: run classify_browser_popins first`);
  const los = JSON.parse(readFileSync(losFile, "utf8"));
  for (const s of summary.shots) {
    const dir = join(raw, `${label}_${s.id}`);
    const seenA = los[s.id]?.appeared ?? [];
    const seenV = los[s.id]?.vanished ?? [];
    const app = s.appeared.map((e, k) => ({ ...e, plain: seenA[k] === true }));
    const van = s.vanished.map((e, k) => ({ ...e, plain: seenV[k] === true }));
    const ring = (e, colour, w) => `<div style="position:absolute;left:${(e.screen[0] / 1280) * 100}%;top:${(e.screen[1] / 720) * 100}%;width:${w}px;height:${w}px;margin:-${w / 2}px;border:2px solid ${colour};border-radius:50%"></div>`;
    const cells = s.strip
      .map((step) => {
        const inSecond = (e) => e.step > step - 60 && e.step <= step;
        const a = app.filter(inSecond);
        const v = van.filter(inSecond);
        const list = (l) => l.map((e) => `${e.dist} m${e.plain ? " PLAIN VIEW" : ""}`).join(", ") || "none";
        return `<figure style="margin:4px;width:430px;display:inline-block;vertical-align:top"><div style="position:relative;width:430px"><img style="width:430px;display:block" src="${img(dir, step)}">${a.map((e) => ring(e, e.plain ? "#ff3030" : "#909090", e.plain ? 26 : 18)).join("")}${v.map((e) => ring(e, e.plain ? "#30e0ff" : "#305060", e.plain ? 26 : 18)).join("")}</div><figcaption style="font:11px monospace;color:#ddd">t+${step / 60}s · appeared: ${list(a)}<br>vanished: ${list(v)}</figcaption></figure>`;
      })
      .join("");
    const plainA = app.filter((e) => e.plain).length;
    const plainV = van.filter((e) => e.plain).length;
    const head = `${label} · ${s.place} · ${s.hour}:00 · Free Roam, chase camera, gfx ${summary.gfx} · ${s.trafficAtStart.total} traffic cars in reach, ${s.trafficAtStart.onScreen} in the view cone · in 6 s: ${s.appeared.length} appeared in the view cone, <b>${plainA} in plain view</b>; ${s.vanished.length} vanished, <b>${plainV} in plain view</b>`;
    const key = `<span style="color:#ff3030">red</span> appeared in plain view · <span style="color:#909090">grey</span> appeared behind cover · <span style="color:#30e0ff">cyan</span> vanished in plain view · <span style="color:#5080a0">dark</span> vanished behind cover`;
    await page.setViewportSize({ width: 1340, height: 400 });
    await page.setContent(`<body style="margin:6px;background:#111"><div style="font:14px sans-serif;color:#fff;margin:4px">${head}</div><div style="font:12px sans-serif;color:#bbb;margin:4px">${key}</div>${cells}</body>`);
    await page.screenshot({ path: join(out, `${label}_${s.id}.png`), fullPage: true });

    // Zoom: nearest plain-view appearance, else nearest behind cover.
    const pick = [...app].sort((x, y) => Number(y.plain) - Number(x.plain) || x.dist - y.dist)[0];
    if (!pick) continue;
    const [cx, cy] = pick.screen;
    const w = 320;
    const h = 180;
    const x0 = Math.max(0, Math.min(1280 - w, cx - w / 2));
    const y0 = Math.max(0, Math.min(720 - h, cy - h / 2));
    const crop = (step, t) =>
      `<figure style="margin:4px;display:inline-block"><div style="width:${w * 2}px;height:${h * 2}px;overflow:hidden;position:relative"><img style="position:absolute;left:${-x0 * 2}px;top:${-y0 * 2}px;width:${1280 * 2}px;image-rendering:pixelated" src="${img(dir, step)}"><div style="position:absolute;left:${(cx - x0) * 2 - 18}px;top:${(cy - y0) * 2 - 18}px;width:32px;height:32px;border:2px solid ${pick.plain ? "#ff3030" : "#909090"};border-radius:50%"></div></div><figcaption style="font:12px monospace;color:#ddd">${t}</figcaption></figure>`;
    const what = pick.plain ? "nearest traffic car that appeared in plain view" : "no car appeared in plain view; nearest that appeared in the view cone (behind cover)";
    await page.setViewportSize({ width: 1320, height: 420 });
    await page.setContent(`<body style="margin:6px;background:#111"><div style="font:14px sans-serif;color:#fff;margin:4px">${label} · ${s.place} · ${s.hour}:00 · ${what}, ${pick.dist} m from the player (2× crop)</div>${crop(pick.before, `frame before (t+${(pick.before / 60).toFixed(2)} s)`)}${crop(pick.frame, `frame it appeared in (t+${(pick.frame / 60).toFixed(2)} s)`)}</body>`);
    await page.screenshot({ path: join(out, `${label}_${s.id}_zoom.png`), fullPage: true });
  }
}
await browser.close();
