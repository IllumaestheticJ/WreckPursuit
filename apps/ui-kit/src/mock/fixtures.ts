/**
 * Fixture registry: every `fixtures/runs/<id>/` directory, inlined at build time.
 * Add a run with `npm run gen:fixtures` (simcli output) or `node scripts/gen-synthetic.mjs`.
 */
import { parseRun, type Run } from "./runs";

const files = import.meta.glob("../../fixtures/runs/*/*.{jsonl,csv,json}", {
  eager: true,
  query: "?raw",
  import: "default",
}) as Record<string, string>;

/** Description of an available run. */
export interface FixtureInfo {
  id: string;
  label: string;
  synthetic: boolean;
}

const LABELS: Record<string, string> = {
  synthetic_pursuit: "Night pursuit (synthetic script)",
  golden_frontal_56kmh: "Frontal 56 km/h into a parked car (simcli)",
  golden_tbone: "T-bone, coupe into sedan (simcli)",
};

function filesOf(id: string): { events: string; metrics: string; summary: string } {
  const get = (name: string) => files[`../../fixtures/runs/${id}/${name}`] ?? "";
  return { events: get("events.jsonl"), metrics: get("metrics.csv"), summary: get("summary.json") || '{"ticks":0,"entities":[]}' };
}

const ids = [...new Set(Object.keys(files).map((k) => k.split("/")[4]))].sort((a, b) =>
  a === "synthetic_pursuit" ? -1 : b === "synthetic_pursuit" ? 1 : a.localeCompare(b),
);

/** Every bundled run. */
export const FIXTURES: FixtureInfo[] = ids.map((id) => {
  const summary = JSON.parse(filesOf(id).summary) as { synthetic?: boolean };
  return { id, label: LABELS[id] ?? id, synthetic: !!summary.synthetic };
});

const cache = new Map<string, Run>();

/** Parse (once) and return a bundled run. Throws for unknown ids. */
export function loadFixture(id: string): Run {
  const hit = cache.get(id);
  if (hit) return hit;
  const info = FIXTURES.find((f) => f.id === id);
  if (!info) throw new Error(`unknown fixture ${id}; have ${FIXTURES.map((f) => f.id).join(", ")}`);
  const run = parseRun(id, info.label, filesOf(id), info.synthetic);
  cache.set(id, run);
  return run;
}
