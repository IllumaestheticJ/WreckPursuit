/**
 * Race lobby / street meet: event card, entrants by role (asymmetric multiplayer: racers vs
 * captain vs chase units), role picker, ready-up with buy-in, countdown.
 */
import type { Role } from "../../generated/contracts";
import { useSend, useSlice } from "../../model/GameModel";
import { CARS } from "../../data/catalog";
import { Badge, Button, Money, Panel, ScreenHead, useEscape } from "../../ui/kit";
import { words } from "../../ui/format";
import "./race.css";

const ROLES: { id: Role; label: string; blurb: string }[] = [
  { id: "Racer", label: "Racer", blurb: "Drive. Win the purse. Don't get caught." },
  { id: "PoliceChase", label: "Chase unit", blurb: "Player-driven cruiser. Box them in." },
  { id: "PoliceCaptain", label: "Captain", blurb: "One per lobby. Map, dispatch, tools." },
  { id: "Spectator", label: "Spectator", blurb: "Watch from any camera." },
];

/** Lobby screen. */
export function Lobby() {
  const send = useSend();
  const lobby = useSlice((s) => s.lobby);
  const cash = useSlice((s) => s.account.cash);
  useEscape(() => send({ type: "lobby.leave" }));
  const me = lobby.entrants.find((e) => e.isLocal);
  const over = me?.classIndex != null && me.classIndex > lobby.maxClassIndex;
  return (
    <div class="wp-screen">
      <ScreenHead kicker={`// STREET MEET · ${lobby.kind.toUpperCase()}`} title={lobby.eventName}>
        <Button size="sm" variant="danger" onClick={() => send({ type: "lobby.leave" })}>
          Leave
        </Button>
      </ScreenHead>
      <div class="wp-lobby">
        <Panel title="Event" class="wp-lobby-event">
          <dl class="wp-kv">
            <dt>Route</dt>
            <dd>{lobby.location}</dd>
            <dt>Laps</dt>
            <dd>{lobby.laps}</dd>
            <dt>Bracket</dt>
            <dd>
              <span class="wp-class-chip">{lobby.bracket}</span> index ≤ {lobby.maxClassIndex}
            </dd>
            <dt>Buy-in</dt>
            <dd>
              <Money value={lobby.buyIn} />
            </dd>
            <dt>Purse</dt>
            <dd>
              <Money value={lobby.purse} tone />
            </dd>
            <dt>Damage</dt>
            <dd>
              <Badge tone={lobby.damageMode === "Arcade" ? "warn" : "danger"}>{lobby.damageMode}</Badge>
            </dd>
            <dt>Police</dt>
            <dd>{lobby.policeEnabled ? <Badge tone="danger">Active</Badge> : <Badge>Off</Badge>}</dd>
          </dl>
          {lobby.countdownS !== null && (
            <div class="wp-lobby-countdown">
              <span class="wp-dim wp-mono">GREEN IN</span>
              <b>{lobby.countdownS}</b>
            </div>
          )}
        </Panel>
        <Panel title="Entrants" tag={`${lobby.entrants.filter((e) => e.role === "Racer").length}/${lobby.maxRacers} RACERS`}>
          <table class="wp-table">
            <thead>
              <tr>
                <th>Player</th>
                <th>Role</th>
                <th>Car</th>
                <th class="num">Index</th>
                <th class="num">Status</th>
              </tr>
            </thead>
            <tbody>
              {lobby.entrants.map((e) => (
                <tr key={e.player} class={e.isLocal ? "is-local" : ""}>
                  <td>
                    {e.crew && <span class="wp-mono wp-dim">[{e.crew}] </span>}
                    <b>{e.name}</b>
                  </td>
                  <td>
                    <Badge tone={e.role === "Racer" ? "ok" : e.role === "Spectator" ? "dim" : "danger"}>{words(e.role)}</Badge>
                  </td>
                  <td>{e.car ? CARS[e.car]?.display_name ?? e.car : <span class="wp-dim">—</span>}</td>
                  <td class="num">{e.classIndex ?? "—"}</td>
                  <td class="num">{e.ready ? <span class="wp-ok">READY</span> : <span class="wp-dim">…</span>}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Panel>
        <Panel title="Your role" class="wp-lobby-roles">
          <div class="wp-role-grid">
            {ROLES.map((r) => {
              const used = lobby.entrants.filter((e) => e.role === r.id && !e.isLocal).length;
              const full = used >= lobby.roleSlots[r.id];
              return (
                <button type="button" key={r.id} class="wp-role" aria-pressed={me?.role === r.id} disabled={full && me?.role !== r.id} onClick={() => send({ type: "lobby.setRole", role: r.id })}>
                  <span class="wp-spread">
                    <b class="wp-upper">{r.label}</b>
                    <span class="wp-mono wp-dim">
                      {used}/{lobby.roleSlots[r.id]}
                    </span>
                  </span>
                  <span class="wp-dim2">{r.blurb}</span>
                </button>
              );
            })}
          </div>
          {over && <p class="wp-danger">Your car's index {me?.classIndex} is over the bracket limit.</p>}
          <div class="wp-row" style={{ justifyContent: "flex-end", marginTop: "0.8em" }}>
            {me?.role === "Racer" && <span class="wp-mono wp-dim">Buy-in charged at the start · cash <Money value={cash} /></span>}
            <Button variant="primary" disabled={over} onClick={() => send({ type: "lobby.setReady", ready: !me?.ready })}>
              {me?.ready ? "Not ready" : "Ready"}
            </Button>
          </div>
        </Panel>
      </div>
    </div>
  );
}
