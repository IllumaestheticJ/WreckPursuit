/** Title / main menu. */
import { useSend, useSlice } from "../../model/GameModel";
import type { ScreenId } from "../../model/types";
import { Badge, Meter, Money, Panel, severityColor } from "../../ui/kit";
import { clock, mmss } from "../../ui/format";
import { HeatPanel } from "../hud/Hud";
import "./menu.css";

const ITEMS: { label: string; hint: string; screen: ScreenId }[] = [
  { label: "Hit the streets", hint: "Free roam the city tonight", screen: "hud" },
  { label: "Street meet", hint: "Lobby · races · pink slips", screen: "lobby" },
  { label: "Garage", hint: "Repair · mods · insurance", screen: "garage" },
  { label: "Market", hint: "Buy and sell cars", screen: "market" },
  { label: "Crews & turf", hint: "Who runs which block", screen: "crewMap" },
  { label: "Police career", hint: "Wear the badge", screen: "police" },
  { label: "Captain's desk", hint: "Command the night shift", screen: "captain" },
  { label: "Replays", hint: "Watch it back", screen: "replay" },
  { label: "Settings", hint: "Controls · assists · accessibility", screen: "settings" },
];

/** Main menu. */
export function MainMenu() {
  const send = useSend();
  const account = useSlice((s) => s.account);
  const active = useSlice((s) => s.garage.cars.find((c) => c.uid === s.garage.activeCar));
  const time = useSlice((s) => s.hud.timeOfDayS);
  const mode = useSlice((s) => s.settings.damageMode);
  const impounded = useSlice((s) => s.impound.entries.length);
  return (
    <div class="wp-screen wp-menu">
      <div class="wp-menu-brand">
        <div class="wp-mono wp-menu-kicker">// CITY UPLINK · {clock(time)} · CH 3</div>
        <h1>
          WRECK<span>PURSUIT</span>
        </h1>
      </div>
      <nav class="wp-menu-list" aria-label="Main menu">
        {ITEMS.map((it, i) => (
          <button type="button" class="wp-menu-item" onClick={() => send({ type: "navigate", screen: it.screen })} key={it.screen} autofocus={i === 0}>
            <span class="wp-mono wp-dim">{String(i + 1).padStart(2, "0")}</span>
            <span class="wp-menu-label">{it.label}</span>
            <span class="wp-menu-hint">{it.hint}</span>
          </button>
        ))}
      </nav>
      <Tonight />
      <aside class="wp-menu-side wp-mono">
        <div>
          CASH <Money value={account.cash} tone />
        </div>
        <div>
          REP <span class="wp-ok">{account.rep.toLocaleString("en-US")}</span>
        </div>
        <div>
          BOUNTY <span class="wp-warn">${account.bounty.toLocaleString("en-US")}</span>
        </div>
        <div>
          RIDE <span class="wp-dim2">{active ? active.name : "none"}</span>
        </div>
        <div class="wp-row" style={{ marginTop: "0.6em" }}>
          <Badge tone={mode === "Simulation" ? "danger" : "warn"}>{mode} damage</Badge>
          {impounded > 0 && <Badge tone="danger">{impounded} in impound</Badge>}
          {account.inDebt && <Badge tone="danger" solid>In debt</Badge>}
        </div>
      </aside>
    </div>
  );
}

/** Middle column: what is happening on the street right now (heat, active car, radio). */
function Tonight() {
  const heat = useSlice((s) => s.hud.heat);
  const car = useSlice((s) => s.garage.cars.find((c) => c.uid === s.garage.activeCar));
  const scanner = useSlice((s) => s.scanner);
  const time = useSlice((s) => s.hud.timeOfDayS);
  const radio = scanner.filter((e) => e.channel === "dispatch" || e.channel === "unit").slice(-6).reverse();
  return (
    <div class="wp-menu-tonight wp-stack">
      <Panel title="Tonight" tag={clock(time)}>
        <HeatPanel heat={heat} />
        {car && (
          <div class="wp-stack" style={{ marginTop: "0.8em" }}>
            <div class="wp-spread">
              <b class="wp-upper">{car.name}</b>
              <span class="wp-mono" style={{ color: severityColor(1 - car.health) }}>
                {Math.round(car.health * 100)}%
              </span>
            </div>
            <Meter value={car.health} color={severityColor(1 - car.health)} label="Active car condition" />
            {car.outstanding.lines.length > 0 && <div class="wp-mono wp-warn" style={{ fontSize: "0.8em" }}>{car.outstanding.lines.length} repairs outstanding</div>}
          </div>
        )}
      </Panel>
      <Panel title="Scanner" tag="CH 3">
        <ul class="wp-feed" style={{ maxHeight: "none" }}>
          {radio.length === 0 && <li class="wp-dim">Quiet night so far.</li>}
          {radio.map((e) => (
            <li key={e.id} data-sev={e.severity} style={{ gridTemplateColumns: "3.4em 1fr" }}>
              <span class="wp-dim">{mmss(e.tS)}</span>
              <span class="wp-ok">{e.text}</span>
            </li>
          ))}
        </ul>
      </Panel>
    </div>
  );
}
