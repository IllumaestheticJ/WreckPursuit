/** Police side: rank & XP, unlocks, career stats, the orders feed and current assignment. */
import { useSend, useSlice } from "../../model/GameModel";
import { Badge, Button, Meter, Panel, ScreenHead, Stat, useEscape } from "../../ui/kit";
import { mmss } from "../../ui/format";
import "./police.css";

/** Police career screen. */
export function PoliceHome() {
  const send = useSend();
  const p = useSlice((s) => s.police);
  useEscape(() => send({ type: "back" }));
  const progress = p.xpToNext === 0 ? 1 : p.xp / (p.xp + p.xpToNext);
  return (
    <div class="wp-screen wp-police-screen">
      <ScreenHead kicker="// METRO POLICE · NIGHT SHIFT" title="Police career" back="back">
        <Button size="sm" onClick={() => send({ type: "navigate", screen: "captain" })}>
          Captain map
        </Button>
      </ScreenHead>
      <div class="wp-police">
        <Panel title="Rank" class="wp-police-rank">
          <div class="wp-police-badge" aria-hidden="true">
            <span>{p.level}</span>
          </div>
          <div class="wp-stack">
            <div class="wp-police-rank-name">{p.rank}</div>
            <Meter value={progress} color="var(--police-b)" segmented label="XP to next rank" />
            <div class="wp-mono wp-dim" style={{ fontSize: "0.8em" }}>
              {p.xp.toLocaleString("en-US")} XP · {p.xpToNext ? `${p.xpToNext.toLocaleString("en-US")} to next rank` : "max rank"}
            </div>
            <div class="wp-grid" style={{ gridTemplateColumns: "repeat(4, auto)", justifyContent: "start", columnGap: "2em" }}>
              <Stat label="Busts" value={p.stats.busts} tone="ok" />
              <Stat label="Escapes" value={p.stats.escapes} tone="warn" />
              <Stat label="Pursuits" value={p.stats.pursuits} />
              <Stat label="Damage caused" value={`$${(p.stats.damageCaused / 1000).toFixed(1)}k`} tone="danger" />
            </div>
          </div>
        </Panel>
        <Panel title="Assignment" tag={p.assignment ? "ACTIVE" : "PATROL"}>
          <div class="wp-police-assignment">{p.assignment ?? "Free patrol — respond to calls in your sector."}</div>
          <p class="wp-dim" style={{ fontSize: "0.8em", marginBottom: 0 }}>
            Orders come from the captain (if a player holds the chair) or from dispatch AI. Clean minutes earn XP; hitting civilians costs it.
          </p>
        </Panel>
        <Panel title="Unlocks" class="wp-police-unlocks">
          <ul>
            {p.unlocks.map((u) => (
              <li key={u.id} data-on={u.unlocked}>
                <span class="wp-mono wp-dim">L{u.level}</span>
                <div>
                  <b>{u.name}</b>
                  <div class="wp-dim" style={{ fontSize: "0.8em" }}>
                    {u.description}
                  </div>
                </div>
                {u.unlocked ? <Badge tone="ok">Unlocked</Badge> : <Badge>Locked</Badge>}
              </li>
            ))}
          </ul>
        </Panel>
        <Panel title="Orders & radio" tag={`${p.orders.length}`} class="wp-police-orders">
          <ul class="wp-feed">
            {p.orders.length === 0 && <li class="wp-dim">Radio quiet.</li>}
            {[...p.orders].reverse().map((o) => (
              <li key={o.id} data-sev={o.severity}>
                <span class="wp-mono wp-dim">{mmss(o.tS)}</span>
                <span>{o.text}</span>
              </li>
            ))}
          </ul>
        </Panel>
      </div>
    </div>
  );
}
