/**
 * Compact chase readout for hosts that keep their own HUD (the testbed): wanted level / heat /
 * bounty (`HeatPanel`), pursuit status line and the latest police radio (at most three lines,
 * fading out; `feed.ts`; the helicopter pilot's lines carry an AIR-1 badge). Transparent, top
 * centre. While police are around, a small radar (the minimap without roads) sits to the right,
 * with the helicopter as Air-1 tethered to whoever it is following.
 */
import { useSlice } from "../../model/GameModel";
import { visibleFeed } from "./feed";
import { airCallsign, HeliBadge } from "./HeliIcon";
import { HeatPanel } from "./Hud";
import { Minimap } from "./Minimap";
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
  const map = useSlice((s) => s.map);
  const hud = useSlice((s) => s.hud);
  const following = hud.blips.some((b) => b.kind === "helicopter" && b.tracking);
  return (
    <div class="wp-hud wp-chase-hud" data-heat={heat.state}>
      <div class="wp-chase-col">
        <HeatPanel heat={heat} />
        <div class="wp-chase-status wp-mono" data-state={heat.state}>
          {STATUS_TEXT[heat.state]}
        </div>
        <ul class="wp-hud-alerts">
          {radio.map(({ entry: e, opacity }) => (
            <li key={e.id} data-sev={e.severity} data-ch={e.channel} data-air={e.air ? "true" : undefined} style={{ opacity }}>
              {e.air && <HeliBadge text={airCallsign(e.text)} />}
              {e.text}
            </li>
          ))}
        </ul>
      </div>
      {hud.blips.length > 0 && (
        <div class="wp-chase-radar">
          <Minimap map={map} pos={hud.pos} headingRad={hud.headingRad} blips={hud.blips} radiusM={320} size="9em" flashing={heat.state === "pursuit"} />
          {following && (
            <div class="wp-chase-radar-note wp-mono">
              <HeliBadge text="AIR-1" /> on you
            </div>
          )}
        </div>
      )}
    </div>
  );
}
