/**
 * Replay transport: play/pause, frame/second stepping, speed, scrubbing with event markers,
 * jump to previous/next event, camera, source. `ReplayControls` is exported on its own so a
 * host can put just the bar under its 3D view.
 */
import { useEffect, useState } from "preact/hooks";
import { useSend, useSlice } from "../../model/GameModel";
import type { ReplayState } from "../../model/types";
import { FIXTURES } from "../../mock/fixtures";
import { Hud } from "../hud/Hud";
import { Button } from "../../ui/kit";
import { mmss } from "../../ui/format";
import "./replay.css";

const SPEEDS = [0.25, 0.5, 1, 2, 4];
const CAMERAS: ReplayState["camera"][] = ["chase", "orbit", "top", "free"];

/** Replay screen: HUD (optional) + transport bar. */
export function ReplayScreen() {
  const [hud, setHud] = useState(true);
  return (
    <div class="wp-replay-screen">
      {hud && <Hud />}
      <div class="wp-replay-dock">
        <ReplayControls showHud={hud} onToggleHud={() => setHud((h) => !h)} />
      </div>
    </div>
  );
}

/** The transport bar alone. Keyboard: Space play/pause, ←/→ step 1 s, ,/. step one tick. */
export function ReplayControls(props: { showHud?: boolean; onToggleHud?: () => void }) {
  const send = useSend();
  const r = useSlice((s) => s.replay);
  const t = r.tick / r.ticksPerSecond;
  const total = r.totalTicks / r.ticksPerSecond;
  const prev = [...r.markers].reverse().find((m) => m.tick < r.tick - 2);
  const next = r.markers.find((m) => m.tick > r.tick + 2);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.target as HTMLElement)?.tagName === "INPUT" || (e.target as HTMLElement)?.tagName === "SELECT") return;
      if (e.code === "Space") {
        e.preventDefault();
        send({ type: r.playing ? "replay.pause" : "replay.play" });
      } else if (e.code === "ArrowLeft") send({ type: "replay.step", ticks: -r.ticksPerSecond });
      else if (e.code === "ArrowRight") send({ type: "replay.step", ticks: r.ticksPerSecond });
      else if (e.code === "Comma") send({ type: "replay.step", ticks: -1 });
      else if (e.code === "Period") send({ type: "replay.step", ticks: 1 });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [r.playing, r.ticksPerSecond]);

  return (
    <div class="wp-replay wp-panel" role="group" aria-label="Replay controls">
      <div class="wp-replay-timeline">
        <input
          type="range"
          min={0}
          max={r.totalTicks}
          step={1}
          value={r.tick}
          aria-label="Replay position"
          onInput={(e) => send({ type: "replay.seek", tick: Number((e.target as HTMLInputElement).value) })}
        />
        <div class="wp-replay-markers" aria-hidden="true">
          {r.markers.map((m, i) => (
            <button
              type="button"
              key={i}
              class="wp-replay-marker"
              data-kind={m.kind}
              style={{ left: `${(m.tick / Math.max(r.totalTicks, 1)) * 100}%` }}
              title={`${mmss(m.tick / r.ticksPerSecond)} ${m.label}`}
              tabIndex={-1}
              onClick={() => send({ type: "replay.seek", tick: m.tick })}
            />
          ))}
        </div>
      </div>
      <div class="wp-replay-row">
        <div class="wp-row">
          <Button size="sm" disabled={!prev} onClick={() => prev && send({ type: "replay.seek", tick: prev.tick })} title="Previous event">
            ⏮
          </Button>
          <Button size="sm" onClick={() => send({ type: "replay.step", ticks: -r.ticksPerSecond })} title="Back 1 s (←)">
            −1s
          </Button>
          <Button size="sm" onClick={() => send({ type: "replay.step", ticks: -1 })} title="Back one tick (,)">
            −1t
          </Button>
          <Button variant="primary" onClick={() => send({ type: r.playing ? "replay.pause" : "replay.play" })} title="Play / pause (Space)">
            {r.playing ? "❚❚ Pause" : "▶ Play"}
          </Button>
          <Button size="sm" onClick={() => send({ type: "replay.step", ticks: 1 })} title="Forward one tick (.)">
            +1t
          </Button>
          <Button size="sm" onClick={() => send({ type: "replay.step", ticks: r.ticksPerSecond })} title="Forward 1 s (→)">
            +1s
          </Button>
          <Button size="sm" disabled={!next} onClick={() => next && send({ type: "replay.seek", tick: next.tick })} title={next ? `Next: ${next.label}` : "No later events"}>
            ⏭
          </Button>
        </div>
        <div class="wp-mono wp-replay-time">
          {mmss(t)}.{String(Math.floor((t % 1) * 100)).padStart(2, "0")} <span class="wp-dim">/ {mmss(total)}</span>
          <span class="wp-dim"> · tick {r.tick}</span>
        </div>
        <div class="wp-row">
          {SPEEDS.map((s) => (
            <Button key={s} size="sm" pressed={r.speed === s} onClick={() => send({ type: "replay.speed", speed: s })}>
              {s}×
            </Button>
          ))}
        </div>
        <div class="wp-row">
          <select class="wp-select" value={r.camera} aria-label="Camera" onChange={(e) => send({ type: "replay.camera", camera: (e.target as HTMLSelectElement).value as ReplayState["camera"] })}>
            {CAMERAS.map((c) => (
              <option key={c} value={c}>
                {c} cam
              </option>
            ))}
          </select>
          <select class="wp-select" value={r.source} aria-label="Replay source" onChange={(e) => send({ type: "replay.load", source: (e.target as HTMLSelectElement).value })}>
            {FIXTURES.map((f) => (
              <option key={f.id} value={f.id}>
                {f.label}
              </option>
            ))}
          </select>
          {props.onToggleHud && (
            <Button size="sm" pressed={props.showHud} onClick={props.onToggleHud}>
              HUD
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}
