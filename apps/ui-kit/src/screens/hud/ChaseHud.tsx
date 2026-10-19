/**
 * Compact chase readout for hosts that keep their own HUD (the testbed): wanted level / heat /
 * bounty (`HeatPanel`), pursuit status line and the latest police radio. Transparent, top centre.
 */
import { useSlice } from "../../model/GameModel";
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
  const radio = scanner.filter((e) => (e.channel === "dispatch" || e.channel === "unit" || e.severity === "alert") && nowS - e.tS < 10).slice(-3);
  return (
    <div class="wp-hud wp-chase-hud" data-heat={heat.state}>
      <div class="wp-chase-col">
        <HeatPanel heat={heat} />
        <div class="wp-chase-status wp-mono" data-state={heat.state}>
          {STATUS_TEXT[heat.state]}
        </div>
        <ul class="wp-hud-alerts">
          {radio.map((e) => (
            <li key={e.id} data-sev={e.severity} data-ch={e.channel}>
              {e.text}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
