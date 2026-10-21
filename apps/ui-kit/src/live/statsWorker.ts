/**
 * Web Worker that measures modded builds on the physics rig (`wp_wasm::car_stats_build_json`),
 * off the main thread (about a second per build). Protocol: in `{ id, defId, mods, mode }`, out
 * `{ id, stats }` or `{ id, error }`; requests run one at a time in arrival order.
 *
 * The wasm package is the testbed's `wasm-pack` output (`apps/testbed-web/src/wasm/pkg`, built
 * by `npm run wasm` there). When it has not been built every request answers with an error and
 * the kit keeps its estimates.
 */
type Pkg = {
  default: () => Promise<unknown>;
  car_stats_build_json: (carId: string, modsJson: string, mode: string) => string;
};

const found = import.meta.glob<Pkg>("../../../testbed-web/src/wasm/pkg/wp_wasm.js");
const load = Object.values(found)[0];
let ready: Promise<Pkg> | null = null;

function pkg(): Promise<Pkg> {
  if (!load) return Promise.reject(new Error("wasm package not built (apps/testbed-web: npm run wasm)"));
  ready ??= load().then(async (m) => {
    await m.default();
    return m;
  });
  return ready;
}

let queue: Promise<void> = Promise.resolve();

self.onmessage = (e: MessageEvent<{ id: number; defId: string; mods: string[]; mode: string }>) => {
  const { id, defId, mods, mode } = e.data;
  queue = queue.then(async () => {
    try {
      const m = await pkg();
      const stats = JSON.parse(m.car_stats_build_json(defId, JSON.stringify(mods), mode));
      self.postMessage({ id, stats });
    } catch (err) {
      self.postMessage({ id, error: String(err) });
    }
  });
};
