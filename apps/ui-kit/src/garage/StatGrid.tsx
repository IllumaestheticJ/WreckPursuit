/** Build stats (PI, mass, 0–100, top speed, grip, brakes), with the change to `after` marked. */
import { Delta } from "../ui/kit";
import type { StatsView } from "./types";

const fmt = {
  pi: (v: number) => v.toFixed(0),
  kg: (v: number) => `${v.toFixed(0)} kg`,
  s: (v: number) => `${v.toFixed(2)} s`,
  kmh: (v: number) => `${v.toFixed(0)} km/h`,
  g: (v: number) => `${v.toFixed(2)} g`,
};

/** Stats of a build; with `after`, each row shows the change from `now`. */
export function StatGrid({ now, after }: { now: StatsView; after?: StatsView | null }) {
  const a = after ?? now;
  return (
    <div class="wp-cg-stats wp-mono">
      <span class="wp-dim">PI</span>
      <span>
        <Delta before={now.class_index} after={a.class_index} format={fmt.pi} /> <span class="wp-dim">({a.bracket})</span>
      </span>
      <span class="wp-dim">Mass</span>
      <Delta before={now.mass_kg} after={a.mass_kg} format={fmt.kg} higherIsBetter={false} epsilon={0.5} />
      <span class="wp-dim">0–100</span>
      <Delta before={now.est_0_100_s} after={a.est_0_100_s} format={fmt.s} higherIsBetter={false} epsilon={0.005} />
      <span class="wp-dim">Top</span>
      <Delta before={now.top_speed_kmh} after={a.top_speed_kmh} format={fmt.kmh} epsilon={0.5} />
      <span class="wp-dim">Grip</span>
      <Delta before={now.lateral_g} after={a.lateral_g} format={fmt.g} epsilon={0.005} />
      <span class="wp-dim">Brakes</span>
      <Delta before={now.brake_g} after={a.brake_g} format={fmt.g} epsilon={0.005} />
    </div>
  );
}
