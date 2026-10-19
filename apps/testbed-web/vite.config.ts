import { defineConfig } from "vite";

// The wasm-pack output lives in src/wasm/pkg (generated, gitignored). Run `npm run wasm`
// (or `just web`, which does it for you) before `npm run dev`.
export default defineConfig({
  server: { port: 5173 },
  build: { target: "es2022" },
});
