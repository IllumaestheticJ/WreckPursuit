/**
 * Compact chase readout for hosts that keep their own HUD (the testbed): wanted level / heat /
 * bounty (`HeatPanel`), pursuit status line and the latest police radio (at most three lines,
 * fading out; `feed.ts`; a helicopter pilot's lines carry an AIR badge). Transparent, top centre.
 * While police are around (the host sends a radar), a small radar sits to the right: police
 * cars, racers and the helicopter as its call sign, tethered to whoever it is following; hidden
 * when the host shows its own minimap. A helicopter on the player adds "AIR <id> on you" to the
 * status line (PR #32).
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
  const onYou = hud.blips.find((b) => b.kind === "helicopter" && b.tracking && b.tracking[0] === hud.pos[0] && b.tracking[1] === hud.pos[1]);
  return (
    <div class="wp-hud wp-chase-hud" data-heat={heat.state}>
      <div class="wp-chase-col">
        <HeatPanel heat={heat} />
        <div class="wp-chase-status wp-mono" data-state={heat.state}>
          {STATUS_TEXT[heat.state]}
          {/* In the status line, above the radio lines (not beside them). */}
          {onYou && (
            <span class="wp-chase-radar-note">
              <HeliBadge text={onYou.label} /> on you
            </span>
          )}
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
      {hud.blips.length > 0 && !hud.radarHidden && (
        <div class="wp-chase-radar">
          <Minimap map={map} pos={hud.pos} headingRad={hud.headingRad} blips={hud.blips} radiusM={320} size="9em" flashing={heat.state === "pursuit"} />
        </div>
      )}
    </div>
  );
}
