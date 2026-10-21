/**
 * Police scanner feed: every dispatch / unit / civil / system line, filterable, newest at the
 * bottom with auto-scroll (pauses when you scroll up).
 */
import { airCallsign, HeliBadge } from "../hud/HeliIcon";
import { useEffect, useRef, useState } from "preact/hooks";
import { useSend, useSlice } from "../../model/GameModel";
import type { ScannerEntry } from "../../model/types";
import { Button, Panel, ScreenHead, useEscape } from "../../ui/kit";
import { mmss } from "../../ui/format";
import "./police.css";

const CHANNELS: ScannerEntry["channel"][] = ["dispatch", "unit", "civil", "system"];

/** Scanner screen. */
export function ScannerFeed() {
  const send = useSend();
  const entries = useSlice((s) => s.scanner);
  const playing = useSlice((s) => s.replay.playing);
  const [on, setOn] = useState<ScannerEntry["channel"][]>(CHANNELS);
  const [follow, setFollow] = useState(true);
  const list = useRef<HTMLUListElement>(null);
  useEscape(() => send({ type: "back" }));
  const shown = entries.filter((e) => on.includes(e.channel));
  useEffect(() => {
    if (follow && list.current) list.current.scrollTop = list.current.scrollHeight;
  }, [shown.length, follow]);
  const toggle = (c: ScannerEntry["channel"]) => setOn((o) => (o.includes(c) ? o.filter((x) => x !== c) : [...o, c]));
  return (
    <div class="wp-screen">
      <ScreenHead kicker="// 154.250 MHz · CH 3" title="Scanner" back="back">
        <span class={playing ? "wp-ok" : "wp-dim"}>{playing ? "● LIVE" : "❚❚ PAUSED"}</span>
      </ScreenHead>
      <Panel title="Feed" tag={`${shown.length} / ${entries.length}`}>
        <div class="wp-row" style={{ marginBottom: "0.6em" }}>
          {CHANNELS.map((c) => (
            <Button key={c} size="sm" pressed={on.includes(c)} onClick={() => toggle(c)}>
              {c}
            </Button>
          ))}
          <span style={{ flex: 1 }} />
          <Button size="sm" pressed={follow} onClick={() => setFollow((f) => !f)}>
            Follow
          </Button>
        </div>
        <ul
          class="wp-feed wp-scanner"
          ref={list}
          aria-live="polite"
          onScroll={(e) => {
            const el = e.currentTarget as HTMLUListElement;
            const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 8;
            if (atBottom !== follow) setFollow(atBottom);
          }}
        >
          {shown.length === 0 && <li class="wp-dim">Static… nothing on the air yet.</li>}
          {shown.map((e) => (
            <li key={e.id} data-sev={e.severity} data-ch={e.channel} data-air={e.air ? "true" : undefined}>
              <span class="wp-mono wp-dim">{mmss(e.tS)}</span>
              <span class="wp-scanner-ch">{e.air ? <HeliBadge text={airCallsign(e.text)} /> : e.channel.toUpperCase()}</span>
              <span>{e.text}</span>
            </li>
          ))}
        </ul>
      </Panel>
    </div>
  );
}
