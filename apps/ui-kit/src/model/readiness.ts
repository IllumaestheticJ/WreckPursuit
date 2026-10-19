/** Empty `wp_police::ToolReadiness` (nothing cooling down, nothing placed). */
import type { ToolReadiness } from "./police";

/** Fresh tool readiness with `wp_police`'s budgets (1 roadblock, 2 spike strips). */
export function initialReadiness(): ToolReadiness {
  return { roadblock_s: 0, spikes_s: 0, helo_s: 0, roadblocks: [0, 1], spikes: [0, 2] };
}
