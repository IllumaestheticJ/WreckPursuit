// Headless car gallery: renders every car's generated mesh (or the named ones) from a few
// angles, day and night, into PNGs — the visual check for style sheets.
//   node tools/assets/gallery.mjs [--out DIR (default: <tmp>/wp-car-gallery)] [--views front34,rear34,side] [--night] [car ...]
// Needs the testbed's node_modules (three, playwright-core) and the pre-installed Chromium.
import { createServer } from "node:http";
import { readFileSync, readdirSync, mkdirSync, existsSync } from "node:fs";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { tmpdir } from "node:os";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const args = process.argv.slice(2);
const opt = (k, d) => {
  const i = args.indexOf(k);
  return i >= 0 ? args.splice(i, 2)[1] : d;
};
const out = resolve(opt("--out", join(tmpdir(), "wp-car-gallery")));
const views = opt("--views", "front34,rear34").split(",");
const nightIdx = args.indexOf("--night");
const night = nightIdx >= 0;
if (night) args.splice(nightIdx, 1);
let cars = args.length ? args : readdirSync(join(repo, "data/panels/meshes")).filter((f) => f.endsWith(".json")).map((f) => f.slice(0, -5)).sort();
mkdirSync(out, { recursive: true });

const types = { ".html": "text/html", ".js": "text/javascript", ".json": "application/json" };
const server = createServer((req, res) => {
  const p = join(repo, decodeURIComponent(new URL(req.url, "http://x").pathname));
  if (!p.startsWith(repo) || !existsSync(p)) {
    res.writeHead(404).end();
    return;
  }
  res.writeHead(200, { "content-type": types[extname(p)] ?? "application/octet-stream" }).end(readFileSync(p));
});
await new Promise((r) => server.listen(0, r));
const port = server.address().port;
const { chromium } = await import(pathToFileURL(join(repo, "apps/testbed-web/node_modules/playwright-core/index.mjs")).href);
const browser = await chromium.launch({
  executablePath: process.env.CHROMIUM ?? "/opt/pw-browsers/chromium",
  args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader", "--ignore-gpu-blocklist"],
});
const page = await (await browser.newContext({ viewport: { width: 960, height: 600 } })).newPage();
page.on("pageerror", (e) => console.error("page error:", String(e)));
await page.goto(`http://localhost:${port}/tools/assets/gallery/index.html`);
await page.waitForFunction(() => window.galleryReady === true, null, { timeout: 30000 });
for (const car of cars) {
  for (const view of views) {
    const info = await page.evaluate(([c, v, n]) => window.showCar(c, v, n), [car, view, night]);
    const file = join(out, `${car}_${view}${night ? "_night" : ""}.png`);
    await page.locator("canvas").screenshot({ path: file });
    console.log(`${file}  (${info.parts} parts, ${info.tris} tris)`);
  }
}
await browser.close();
server.close();
