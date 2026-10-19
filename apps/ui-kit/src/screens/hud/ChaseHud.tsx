/**
 * Compact chase readout for hosts that keep their own HUD (the testbed): wanted level / heat /
 * bounty (`HeatPanel`), pursuit status line and the latest police radio (at most three lines,
 * fading out; `feed.ts`). Transparent, top centre.
 */
import { useSlice } from "../../model/GameModel";
import { visibleFeed } from "./feed";
import { HeatPanel } from "./Hud";
import "./hud.css";

const STATUS_TEXT = {
  clear: "No police interest",
  noticed: "Wanted — police are looking for you",
  pursuit: "In pursuit",
  cooldown: "Lost them — searching",
} as const;

/** The chase readout screen. */
export function ChaseHud() {
  const heat = useSlice((s) => s.hud.heat);
  const scanner = useSlice((s) => s.scanner);
  const tick = useSlice((s) => s.replay.tick);
  const tps = useSlice((s) => s.replay.ticksPerSecond);
  const nowS = tick / tps;
  const radio = visibleFeed(scanner, nowS, (e) => e.channel === "dispatch" || e.channel === "unit" || e.severity === "alert");
  return (
    <div class="wp-hud wp-chase-hud" data-heat={heat.state}>
      <div class="wp-chase-col">
        <HeatPanel heat={heat} />
        <div class="wp-chase-status wp-mono" data-state={heat.state}>
          {STATUS_TEXT[heat.state]}
        </div>
        <ul class="wp-hud-alerts">
          {radio.map(({ entry: e, opacity }) => (
            <li key={e.id} data-sev={e.severity} data-ch={e.channel} style={{ opacity }}>
              {e.text}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
