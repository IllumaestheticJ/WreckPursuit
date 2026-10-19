import { defineConfig, type Plugin } from "vite";
import { readdirSync, readFileSync, statSync, existsSync } from "node:fs";
import { join, relative, resolve, sep } from "node:path";
import type { IncomingMessage, ServerResponse } from "node:http";

// The wasm-pack output lives in src/wasm/pkg (generated, gitignored). Run `npm run wasm`
// (or `just web`, which does it for you) before `npm run dev`.

const repo = resolve(__dirname, "../..");

/**
 * Folders served read-only to the testbed: URL prefix → repo folders (searched in order).
 * Scenarios come from simcli's canonical folder plus the testbed's own (older dialect).
 */
const mounts: Record<string, string[]> = {
  "/scenarios/": [join(repo, "apps/simcli/scenarios"), join(repo, "apps/testbed-web/scenarios")],
  "/data/": [join(repo, "data")],
};

function listFiles(dir: string, ext: string[]): string[] {
  const out: string[] = [];
  const walk = (d: string) => {
    for (const name of readdirSync(d).sort()) {
      const p = join(d, name);
      if (statSync(p).isDirectory()) walk(p);
      else if (ext.some((e) => name.endsWith(e))) out.push(relative(dir, p).split(sep).join("/"));
    }
  };
  if (existsSync(dir)) walk(dir);
  return out;
}

/**
 * Serves `apps/simcli/scenarios/**` and `apps/testbed-web/scenarios/**` at `/scenarios/` and `data/**` at `/data/`, plus JSON
 * listings at `/scenarios/index.json` (all `.ron`) and `/data/city/index.json` (all `.glb`).
 * Works for both `vite` (dev) and `vite preview`.
 */
function repoFiles(): Plugin {
  const handler = (req: IncomingMessage, res: ServerResponse, next: () => void) => {
    const url = decodeURIComponent((req.url ?? "").split("?")[0]);
    for (const [prefix, dirs] of Object.entries(mounts)) {
      if (!url.startsWith(prefix)) continue;
      const rel = url.slice(prefix.length);
      if (rel === "index.json" && prefix === "/scenarios/") {
        res.setHeader("Content-Type", "application/json");
        res.end(JSON.stringify([...new Set(dirs.flatMap((d) => listFiles(d, [".ron"])))].sort()));
        return;
      }
      if (rel === "city/index.json" && prefix === "/data/") {
        res.setHeader("Content-Type", "application/json");
        res.end(JSON.stringify(listFiles(join(dirs[0], "city"), [".glb"])));
        return;
      }
      for (const dir of dirs) {
        const file = resolve(dir, rel);
        if (!file.startsWith(dir) || !existsSync(file) || statSync(file).isDirectory()) continue;
        res.setHeader("Cache-Control", "no-cache");
        res.end(readFileSync(file));
        return;
      }
      break;
    }
    next();
  };
  return {
    name: "wp-repo-files",
    configureServer(server) {
      server.middlewares.use(handler);
    },
    configurePreviewServer(server) {
      server.middlewares.use(handler);
    },
  };
}

export default defineConfig({
  plugins: [repoFiles()],
  server: { port: 5173 },
  preview: { port: 4173 },
  build: {
    target: "es2022",
    // net.html: multiplayer client (workstream F).
    rollupOptions: { input: { main: "index.html", net: "net.html" } },
  },
});
