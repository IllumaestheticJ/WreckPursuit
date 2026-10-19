// Run scenarios inside the wasm32 build of simcli's runner and print hashes.txt for each.
//
//   node tools/wasm-determinism/run.mjs <wasm> <bundle.json> <scenario.ron>...
//
// <wasm> is target/wasm32-unknown-unknown/release/wasm_determinism.wasm and <bundle.json> comes
// from `simcli bundle`. See tools/wasm-determinism/README.md; `compare.sh` wraps it all.
import { readFileSync } from "node:fs";

const [wasmPath, bundlePath, ...scenarios] = process.argv.slice(2);
if (!wasmPath || !bundlePath || scenarios.length === 0) {
  console.error("usage: run.mjs <wasm> <bundle.json> <scenario.ron>...");
  process.exit(2);
}

// Any import the module asks for is a bug in determinism terms (it would reach outside the
// sandbox), so every import throws when called.
const module = await WebAssembly.compile(readFileSync(wasmPath));
const imports = {};
for (const imp of WebAssembly.Module.imports(module)) {
  imports[imp.module] ??= {};
  imports[imp.module][imp.name] = () => {
    throw new Error(`wasm called host import ${imp.module}.${imp.name}`);
  };
}
const { exports: wasm } = await WebAssembly.instantiate(module, imports);

const enc = new TextEncoder();
const dec = new TextDecoder();
function put(text) {
  const bytes = enc.encode(text);
  const ptr = wasm.wp_alloc(bytes.length);
  new Uint8Array(wasm.memory.buffer, ptr, bytes.length).set(bytes);
  return [ptr, bytes.length];
}

const bundle = put(readFileSync(bundlePath, "utf8"));
let failed = false;
for (const path of scenarios) {
  const sc = put(readFileSync(path, "utf8"));
  const status = wasm.wp_run(sc[0], sc[1], bundle[0], bundle[1]);
  wasm.wp_free(sc[0], sc[1]);
  const out = dec.decode(
    new Uint8Array(wasm.memory.buffer, wasm.wp_result_ptr(), wasm.wp_result_len()),
  );
  if (status !== 0) {
    console.error(`${path}: ${out}`);
    failed = true;
    continue;
  }
  process.stdout.write(out);
}
process.exit(failed ? 1 : 0);
