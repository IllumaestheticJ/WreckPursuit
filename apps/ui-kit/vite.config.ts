import { defineConfig } from "vitest/config";

// The demo page is the dev server root (index.html → demo/main.tsx). The kit reads real game data
// from ../../data (cars, mods), so the dev server must be allowed to serve the repo root.
export default defineConfig({
  esbuild: { jsx: "automatic", jsxImportSource: "preact" },
  server: { port: 5180, fs: { allow: [new URL("../..", import.meta.url).pathname] } },
  // The demo inlines fixtures, car and mod data on purpose, so the bundle is large.
  build: { outDir: "dist", sourcemap: true, chunkSizeWarningLimit: 2000 },
  test: { include: ["src/**/*.test.ts"], environment: "node" },
});
