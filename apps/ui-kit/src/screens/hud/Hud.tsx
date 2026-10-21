/**
 * In-race HUD overlay. Transparent background: it sits on top of the 3D view.
 *
 * Layout (corners keep the centre clear for driving):
 *   top-left: race position / checkpoints / gaps      top-right: heat & wanted, minimap
 *   left:     scanner alerts                            right:     damage silhouette + components
 *   bottom-centre: speed / gear / rpm, assist badges
 */
import type { Component } from "../../generated/contracts";
import { useSlice } from "../../model/GameModel";
import { wantedLevelNumber } from "../../model/proposed";
import type { HeatHud, HudState, RaceHud, ScannerEntry } from "../../model/types";
import { clock, gap, gearLabel, raceTime, speed, words } from "../../ui/format";
import { Badge, Meter, severityColor } from "../../ui/kit";
import { DamageSilhouette } from "./DamageSilhouette";
import { visibleFeed } from "./feed";
import { Minimap } from "./Minimap";
import "./hud.css";

/** The HUD screen. */
export function Hud() {
  const hud = useSlice((s) => s.hud);
  const units = useSlice((s) => s.settings.units);
  const scanner = useSlice((s) => s.scanner);
  const map = useSlice((s) => s.map);
  const tick = useSlice((s) => s.replay.tick);
  const tps = useSlice((s) => s.replay.ticksPerSecond);
  const nowS = tick / tps;
  return (
    <div class="wp-hud" data-heat={hud.heat.state}>
      <div class="wp-hud-tl">{hud.race && <RacePanel race={hud.race} />}</div>
      <div class="wp-hud-tr">
        <HeatPanel heat={hud.heat} />
        <div class="wp-hud-minimap">
          <Minimap map={map} pos={hud.pos} headingRad={hud.headingRad} blips={hud.blips} flashing={hud.heat.state === "pursuit"} />
          <span class="wp-mono wp-dim">{clock(hud.timeOfDayS)}</span>
        </div>
      </div>
      <div class="wp-hud-l">
        <Alerts entries={scanner} nowS={nowS} />
      </div>
      <div class="wp-hud-r">
        <DamagePanel hud={hud} />
      </div>
      <div class="wp-hud-b">
        <Speedo hud={hud} units={units} />
        <AssistBadges hud={hud} />
      </div>
    </div>
  );
}

function RacePanel({ race }: { race: RaceHud }) {
  return (
    <div class="wp-hud-race">
      <div class="wp-hud-pos">
        <b>{race.position}</b>
        <span>/{race.of}</span>
      </div>
      <div class="wp-hud-race-rows wp-mono">
        <div>
          <span class="wp-dim">LAP</span> {race.lap}/{race.laps}
        </div>
        <div>
          <span class="wp-dim">CP</span> {race.checkpoint}/{race.checkpoints}
        </div>
        <div>
          <span class="wp-dim">TIME</span> {raceTime(race.elapsedS)}
        </div>
        <div>
          <span class="wp-dim">AHEAD</span> <span class={race.gapAheadS === null ? "wp-ok" : "wp-warn"}>{race.gapAheadS === null ? "LEADING" : gap(race.gapAheadS)}</span>
        </div>
        <div>
          <span class="wp-dim">BEHIND</span> <span>{gap(race.gapBehindS)}</span>
        </div>
      </div>
    </div>
  );
}

const STATE_LABEL: Record<HeatHud["state"], string> = {
  clear: "CLEAR",
  noticed: "NOTICED",
  pursuit: "PURSUIT",
  cooldown: "COOLDOWN",
};

/** Wanted level, heat within level, pursuit state, cooldown ring, bounty. */
export function HeatPanel({ heat }: { heat: HeatHud }) {
  const level = wantedLevelNumber(heat.level);
  return (
    <div class="wp-hud-heat" data-state={heat.state} aria-label={`Wanted level ${level}, ${STATE_LABEL[heat.state]}`}>
      <div class="wp-hud-heat-top">
        <span class="wp-hud-heat-state">{STATE_LABEL[heat.state]}</span>
        <span class="wp-hud-stars" aria-hidden="true">
          {[1, 2, 3, 4, 5].map((i) => (
            <i key={i} data-on={i <= level} />
          ))}
        </span>
      </div>
      {heat.state === "cooldown" ? (
        <div>
          <Meter value={heat.cooldown} color="var(--info)" segmented label="Cooldown" />
          <div class="wp-mono wp-dim wp-hud-small">SEARCHING… {Math.round(heat.cooldown * 100)}%</div>
        </div>
      ) : (
        // Without a heat value (event-only sources) the bar shows the level band instead.
        <Meter value={level === 0 ? 0 : heat.heatKnown === false ? level / 5 : heat.heat} color={level >= 4 ? "var(--danger)" : "var(--warn)"} segmented label="Heat" />
      )}
      <div class="wp-spread wp-mono wp-hud-small">
        <span>
          <span class="wp-dim">UNITS</span> {heat.unitsInPursuit}
        </span>
        <span>
          <span class="wp-dim">BOUNTY</span> {heat.bountyKnown === false ? <span class="wp-dim">—</span> : <span class="wp-cash">${heat.bounty.toLocaleString("en-US")}</span>}
        </span>
      </div>
    </div>
  );
}

function Alerts({ entries, nowS }: { entries: ScannerEntry[]; nowS: number }) {
  // At most three recent lines, newest at the bottom, fading out (feed.ts).
  const recent = visibleFeed(entries, nowS);
  return (
    <ul class="wp-hud-alerts" aria-live="polite">
      {recent.map(({ entry: e, opacity }) => (
        <li key={e.id} data-sev={e.severity} data-ch={e.channel} style={{ opacity }}>
          <span class="wp-hud-alert-ch">{e.channel === "dispatch" ? "DISP" : e.channel === "unit" ? "UNIT" : e.channel === "civil" ? "CIV" : "SYS"}</span>
          {e.text}
        </li>
      ))}
    </ul>
  );
}

const COMPONENT_ORDER: Component[] = [
  "Engine",
  "Radiator",
  "Gearbox",
  "SteeringRack",
  "SuspensionFL",
  "SuspensionFR",
  "SuspensionRL",
  "SuspensionRR",
  "BrakeFL",
  "BrakeFR",
  "BrakeRL",
  "BrakeRR",
  "FuelTank",
  "Battery",
  "Driveshaft",
  "Exhaust",
  "Lights",
  "WheelFL",
  "WheelFR",
  "WheelRL",
  "WheelRR",
];

function DamagePanel({ hud }: { hud: HudState }) {
  // Show the worst damaged components (max 6), failed first.
  const damaged = COMPONENT_ORDER.filter((c) => (hud.components[c] ?? 1) < 0.999).sort((a, b) => (hud.components[a] ?? 1) - (hud.components[b] ?? 1)).slice(0, 6);
  return (
    <div class="wp-hud-damage">
      <div class="wp-spread wp-mono wp-hud-small">
        <span class="wp-dim">CONDITION</span>
        <span style={{ color: severityColor(1 - hud.health) }}>{Math.round(hud.health * 100)}%</span>
      </div>
      <DamageSilhouette regions={hud.regions} detached={hud.detached} components={hud.components} wheelsDetached={hud.wheelsDetached} totalLoss={hud.totalLoss} width="6.6em" />
      <ul class="wp-hud-components">
        {damaged.length === 0 && <li class="wp-dim">All systems nominal</li>}
        {damaged.map((c) => {
          const h = hud.components[c] ?? 1;
          const failed = hud.failed.includes(c);
          return (
            <li key={c} data-failed={failed}>
              <span>{words(c)}</span>
              {failed ? <b class="wp-danger">FAIL</b> : <span style={{ color: severityColor(1 - h) }}>{Math.round(h * 100)}</span>}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function Speedo({ hud, units }: { hud: HudState; units: "metric" | "imperial" }) {
  const sp = speed(hud.speedMps, units);
  const frac = Math.min(1, hud.engineRpm / (hud.redlineRpm * 1.08));
  const redFrom = 1 / 1.08;
  const segs = 28;
  return (
    <div class="wp-hud-speedo">
      <div class="wp-hud-rpm" aria-label={`${Math.round(hud.engineRpm)} rpm`}>
        {Array.from({ length: segs }, (_, i) => {
          const at = (i + 1) / segs;
          const on = at <= frac;
          const red = at > redFrom * 0.94;
          return <i key={i} data-on={on} data-red={red} />;
        })}
      </div>
      <div class="wp-hud-speed-row">
        <div class="wp-hud-speed">
          {sp.value}
          <small>{sp.unit}</small>
        </div>
        <div class="wp-hud-gear" data-shift={hud.engineRpm > hud.redlineRpm * 0.97}>
          {gearLabel(hud.gear)}
        </div>
      </div>
      <div class="wp-mono wp-dim wp-hud-small">{Math.round(hud.engineRpm).toLocaleString("en-US")} RPM</div>
    </div>
  );
}

function AssistBadges({ hud }: { hud: HudState }) {
  const a = hud.assists;
  // TCS / ABS show their level (Off / Half / Full); a legacy bool reads as Full / Off.
  const lvl = (v: unknown) => (v === true ? "Full" : v === false ? "Off" : String(v));
  const items: [string, boolean][] = [
    [lvl(a.abs) === "Full" ? "ABS" : `ABS ${lvl(a.abs).toUpperCase()}`, lvl(a.abs) !== "Off"],
    [lvl(a.tcs) === "Full" ? "TCS" : `TCS ${lvl(a.tcs).toUpperCase()}`, lvl(a.tcs) !== "Off"],
    ["ESC", a.esc],
    ["STEER", a.countersteer_help > 0 || a.steer_speed_sensitivity > 0],
    ["AUTO", a.auto_gears],
  ];
  return (
    <div class="wp-row wp-hud-assists">
      {items.map(([k, on]) => (
        <Badge key={k} tone={on ? "ok" : "dim"}>
          {k}
        </Badge>
      ))}
      <Badge tone={hud.damageMode === "Simulation" ? "danger" : "warn"} title="Damage mode">
        {hud.damageMode === "Simulation" ? "SIM DMG" : "ARCADE DMG"}
      </Badge>
    </div>
  );
}
