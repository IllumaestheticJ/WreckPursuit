/**
 * Garage workshop overlay: the screen around the 3D bay (the host draws the car and the room;
 * see apps/testbed-web/src/workshop). Patterned on a hands-on car-building garage: the car is the
 * screen, panels sit at the edges and the middle stays clear for the mouse.
 *
 * - Top bar: car, cash, what is missing before the car can drive, back to the garage, drive.
 * - Left toolbar: camera views (overview, engine bay, underside, wheels), the lift, the parts
 *   catalog and the car's part list.
 * - Right: the part card of the selected slot (condition, what is fitted, remove; when empty, the
 *   inventory parts that fit and the catalog for that slot).
 * - Bottom: the inventory strip; choosing a part lights up where it fits.
 * - Engine stand: a loose engine (taken out whole) stripped and built part by part, in order.
 *
 * Every action goes through `WorkshopApi` and redraws from the screen it returns; the overlay
 * holds no game state of its own.
 */
import { render } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { Badge, Button, Meter, Money, Panel, severityColor, Tabs } from "../ui/kit";
import type { CatalogEntry, InventoryView, PartGroup, SlotView, StandView, WorkshopApi, WorkshopScreen } from "./types";
import "./garage.css";

/** Camera presets the host knows. */
export type WorkshopCam = "overview" | "engine" | "under" | "wheels";

/** What the host (3D side) does for the overlay. */
export interface WorkshopHost {
  /** Highlight these slots on the car (empty = none). */
  highlight(slots: string[]): void;
  camera(preset: WorkshopCam): void;
  lift(up: boolean): void;
  /** The car changed (rebuild the 3D view). */
  changed(screen: WorkshopScreen): void;
  back(): void;
  drive(): void;
  /** Display name of the car. */
  carName: string;
}

const GROUPS: PartGroup[] = [
  "Body", "Glass", "Aero", "Protection", "Engine", "Cooling", "Electronics", "Fuel",
  "Drivetrain", "Wheels", "Brakes", "Suspension", "Steering", "Interior",
];

const pct = (v: number) => `${Math.round(v * 100)}%`;

function conditionBar(c: number) {
  return <Meter value={c} color={severityColor(1 - c)} />;
}

/** Slots that are on the car now: parts inside an assembly only while the assembly is fitted. */
function onCar(screen: WorkshopScreen): SlotView[] {
  const fitted = new Set(screen.slots.filter((s) => s.fitted).map((s) => s.id));
  return screen.slots.filter((s) => !s.parent || fitted.has(s.parent));
}

/** Shared state between the 3D side and the overlay (selection, hover). */
export class WorkshopState {
  selected: string | null = null;
  hover: string | null = null;
  private listeners = new Set<() => void>();
  set(patch: Partial<Pick<WorkshopState, "selected" | "hover">>): void {
    Object.assign(this, patch);
    for (const l of this.listeners) l();
  }
  subscribe(l: () => void): () => void {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  }
}

function useWorkshopState(s: WorkshopState) {
  const [, tick] = useState(0);
  useEffect(() => s.subscribe(() => tick((n) => n + 1)), [s]);
  return s;
}

function PartCard(props: {
  slot: SlotView;
  screen: WorkshopScreen;
  run: (f: () => WorkshopScreen) => void;
  api: WorkshopApi;
  slotName: (id: string) => string;
  openCatalog: (kind: string) => void;
}) {
  const { slot, screen, run, api } = props;
  const f = slot.fitted;
  const fits = screen.inventory.filter((p) => p.fits.includes(slot.id));
  const stock = screen.catalog.find((c) => c.kind === slot.kind && c.grade === null);
  return (
    <Panel
      title={slot.name}
      tag={<span class="wp-dim">{slot.parent ? `inside ${props.slotName(slot.parent).toLowerCase()}` : slot.group}</span>}
      class="wp-ws-card"
    >
      <div data-testid="ws-card" data-slot={slot.id} style={{ display: "grid", gap: "0.5em" }}>
        {f ? (
          <>
            <div class="wp-row" style={{ justifyContent: "space-between" }}>
              <span>{f.grade ? f.name : "Stock part"}</span>
              {f.tier > 0 && <Badge tone="info">tier {f.tier}</Badge>}
            </div>
            <div class="wp-row" style={{ gap: "0.6em" }}>
              <span class="wp-dim wp-mono">COND</span>
              <div style={{ flex: 1 }}>{conditionBar(f.condition)}</div>
              <span class="wp-mono">{pct(f.condition)}</span>
            </div>
            <Button
              variant="primary"
              disabled={!!slot.remove_blocker}
              title={slot.remove_blocker ?? "take it off into the inventory"}
              onClick={() => run(() => api.remove(slot.id))}
            >
              Remove
            </Button>
            {slot.remove_blocker && <span class="wp-warn">{slot.remove_blocker}</span>}
          </>
        ) : (
          <>
            <div class="wp-row" style={{ justifyContent: "space-between" }}>
              <span class="wp-dim">Empty</span>
              {slot.required && <Badge tone="danger">needed to drive</Badge>}
            </div>
            {slot.blocked_by.length > 0 && (
              <span class="wp-warn">take off first: {slot.blocked_by.map(props.slotName).join(", ")}</span>
            )}
            {fits.length > 0 ? (
              fits.map((p) => (
                <Button key={p.part.uid} onClick={() => run(() => api.fit(slot.id, p.part.uid))} title="fit this part">
                  Fit {p.name} ({pct(p.part.condition)})
                </Button>
              ))
            ) : (
              <span class="wp-dim">No part in the inventory fits here.</span>
            )}
            <div class="wp-row" style={{ gap: "0.4em", flexWrap: "wrap" }}>
              {stock && (
                <Button
                  size="sm"
                  disabled={stock.price > screen.cash}
                  onClick={() => run(() => api.buy(stock.kind, null))}
                  title="a new stock part into the inventory"
                >
                  Buy stock <Money value={stock.price} />
                </Button>
              )}
              <Button size="sm" onClick={() => props.openCatalog(slot.kind)}>
                Catalog
              </Button>
            </div>
          </>
        )}
      </div>
    </Panel>
  );
}

function Catalog(props: {
  screen: WorkshopScreen;
  kind: string | null;
  run: (f: () => WorkshopScreen) => void;
  api: WorkshopApi;
  onClose: () => void;
}) {
  const groups = GROUPS.filter((g) => props.screen.catalog.some((c) => c.group === g));
  const first = props.kind ? props.screen.catalog.find((c) => c.kind === props.kind)?.group : undefined;
  const [group, setGroup] = useState<PartGroup>(first ?? groups[0] ?? "Body");
  const [kind, setKind] = useState<string | null>(props.kind);
  const rows = props.screen.catalog.filter((c) => c.group === group && (!kind || c.kind === kind));
  const byKind = new Map<string, CatalogEntry[]>();
  for (const r of rows) byKind.set(r.kind, [...(byKind.get(r.kind) ?? []), r]);
  return (
    <div class="wp-ws-modal" data-testid="ws-catalog">
      <Panel
        title="Parts catalog"
        tag={
          <span class="wp-row" style={{ gap: "0.6em" }}>
            <Money value={props.screen.cash} tone />
            <Button size="sm" onClick={props.onClose} hotkey="Esc">
              Close
            </Button>
          </span>
        }
      >
        <Tabs tabs={groups.map((g) => ({ id: g, label: g }))} value={group} onChange={(g) => (setGroup(g), setKind(null))} />
        {kind && (
          <div class="wp-row" style={{ margin: "0.5em 0", gap: "0.5em" }}>
            <span class="wp-dim">Showing parts for one slot.</span>
            <Button size="sm" variant="ghost" onClick={() => setKind(null)}>
              Show all {group}
            </Button>
          </div>
        )}
        <div class="wp-ws-catalog">
          {[...byKind].map(([k, entries]) => (
            <div key={k} class="wp-ws-catgroup">
              <div class="wp-dim wp-mono wp-upper">{entries[0].slot_name}</div>
              {entries.map((c) => (
                <div key={c.grade ?? "stock"} class="wp-ws-catrow" data-testid="ws-catrow" data-kind={c.kind} data-grade={c.grade ?? ""}>
                  <span>
                    {c.name}
                    {c.count > 1 && <span class="wp-dim"> ×{c.count}</span>}
                    {c.tier > 0 && <span class="wp-dim"> · T{c.tier}</span>}
                  </span>
                  <Money value={c.price} />
                  <Button
                    size="sm"
                    variant="primary"
                    disabled={!!c.reason || c.price > props.screen.cash}
                    title={c.reason ?? "into the inventory"}
                    onClick={() => props.run(() => props.api.buy(c.kind, c.grade))}
                  >
                    Buy
                  </Button>
                </div>
              ))}
            </div>
          ))}
        </div>
      </Panel>
    </div>
  );
}

function PartsList(props: { screen: WorkshopScreen; state: WorkshopState; onClose: () => void }) {
  return (
    <div class="wp-ws-list" data-testid="ws-partlist">
      <Panel title="Car parts" tag={<Button size="sm" onClick={props.onClose}>Close</Button>}>
        {GROUPS.map((g) => {
          const slots = onCar(props.screen).filter((s) => s.group === g);
          if (!slots.length) return null;
          return (
            <div key={g} style={{ marginBottom: "0.5em" }}>
              <div class="wp-dim wp-mono wp-upper">{g}</div>
              {slots.map((s) => (
                <button
                  type="button"
                  key={s.id}
                  class="wp-ws-slot"
                  style={s.parent ? { paddingLeft: "1.4em" } : undefined}
                  aria-selected={props.state.selected === s.id}
                  onClick={() => props.state.set({ selected: s.id })}
                >
                  <span>{s.name}</span>
                  <span class="wp-mono" style={{ color: s.fitted ? severityColor(1 - s.fitted.condition) : "var(--dim)" }}>
                    {s.fitted ? pct(s.fitted.condition) : s.required ? "MISSING" : "—"}
                  </span>
                </button>
              ))}
            </div>
          );
        })}
      </Panel>
    </div>
  );
}

/** One loose engine on the stand: its internals in order, strip and build. */
function Stand(props: { stand: StandView; screen: WorkshopScreen; run: (f: () => WorkshopScreen) => void; api: WorkshopApi }) {
  const { stand, screen, run, api } = props;
  const name = (id: string) => stand.slots.find((s) => s.id === id)?.name ?? id;
  return (
    <div class="wp-ws-stand" data-testid="ws-stand" data-uid={stand.uid}>
      <div class="wp-row" style={{ justifyContent: "space-between" }}>
        <strong>{stand.name}</strong>
        {stand.missing.length > 0 ? <Badge tone="warn">{stand.missing.length} parts out</Badge> : <Badge tone="ok">complete</Badge>}
      </div>
      {stand.slots.map((s) => {
        const fits = screen.inventory.filter((p) => p.stand_fits.some((f) => f.assembly === stand.uid && f.slot === s.id));
        const stock = screen.catalog.find((c) => c.kind === s.kind && c.grade === null);
        return (
          <div key={s.id} class="wp-ws-standrow" data-testid="ws-stand-slot" data-slot={s.id}>
            <span>
              {s.name}
              {s.fitted && <span class="wp-dim"> · {s.fitted.grade ? s.fitted.name : "stock"}</span>}
            </span>
            {s.fitted ? (
              <span class="wp-row" style={{ gap: "0.4em" }}>
                <span class="wp-mono" style={{ color: severityColor(1 - s.fitted.condition) }}>
                  {pct(s.fitted.condition)}
                </span>
                <Button
                  size="sm"
                  disabled={!!s.remove_blocker}
                  title={s.remove_blocker ?? "take it out into the inventory"}
                  onClick={() => run(() => api.strip(stand.uid, s.id))}
                >
                  Remove
                </Button>
              </span>
            ) : (
              <span class="wp-row" style={{ gap: "0.4em", flexWrap: "wrap" }}>
                {fits.map((p) => (
                  <Button key={p.part.uid} size="sm" variant="primary" onClick={() => run(() => api.build(stand.uid, s.id, p.part.uid))}>
                    Fit {p.name} ({pct(p.part.condition)})
                  </Button>
                ))}
                {fits.length === 0 && screen.inventory.some((p) => p.part.kind === s.kind) && <span class="wp-dim">in the inventory</span>}
                {fits.length === 0 && stock && !screen.inventory.some((p) => p.part.kind === s.kind) && (
                  <Button size="sm" disabled={stock.price > screen.cash} onClick={() => run(() => api.buy(stock.kind, null))} title="a new stock part">
                    Buy <Money value={stock.price} />
                  </Button>
                )}
                {s.blocked_by.length > 0 && <span class="wp-dim">after: {s.blocked_by.map(name).join(", ")}</span>}
              </span>
            )}
          </div>
        );
      })}
    </div>
  );
}

function EngineStand(props: { screen: WorkshopScreen; run: (f: () => WorkshopScreen) => void; api: WorkshopApi; onClose: () => void }) {
  return (
    <div class="wp-ws-modal" data-testid="ws-engine-stand">
      <Panel
        title="Engine stand"
        tag={
          <Button size="sm" onClick={props.onClose} hotkey="Esc">
            Close
          </Button>
        }
      >
        {props.screen.stands.length === 0 ? (
          <p class="wp-dim">No loose engine. Take the engine out of the car (clear the bay first) to strip it here.</p>
        ) : (
          props.screen.stands.map((st) => <Stand key={st.uid} stand={st} screen={props.screen} run={props.run} api={props.api} />)
        )}
        <p class="wp-dim">Parts come out top down: manifolds, valve cover, timing, cams, valves, head, gasket, pistons, rods, crank.</p>
      </Panel>
    </div>
  );
}

function InventoryStrip(props: {
  screen: WorkshopScreen;
  run: (f: () => WorkshopScreen) => void;
  api: WorkshopApi;
  host: WorkshopHost;
  state: WorkshopState;
}) {
  const [picked, setPicked] = useState<number | null>(null);
  const item = props.screen.inventory.find((p) => p.part.uid === picked) ?? null;
  useEffect(() => {
    props.host.highlight(item ? item.fits : props.state.selected ? [props.state.selected] : []);
  }, [item, props.state.selected]);
  const tile = (p: InventoryView) => (
    <button
      type="button"
      key={p.part.uid}
      class="wp-ws-tile"
      data-testid="ws-tile"
      data-uid={p.part.uid}
      aria-selected={picked === p.part.uid}
      onClick={() => setPicked(picked === p.part.uid ? null : p.part.uid)}
      title={p.reason ?? `fits: ${p.fits.join(", ")}`}
    >
      <span class="wp-ws-tile-name">{p.name}</span>
      <span class="wp-dim wp-mono">{p.part.kind.replace(/_/g, " ")}</span>
      {conditionBar(p.part.condition)}
    </button>
  );
  return (
    <div class="wp-ws-strip">
      <div class="wp-ws-tiles" data-testid="ws-inventory">
        {props.screen.inventory.length === 0 ? <span class="wp-dim">Inventory empty: parts you take off or buy land here.</span> : props.screen.inventory.map(tile)}
      </div>
      {item && (
        <div class="wp-ws-tile-actions">
          {item.fits.map((s) => (
            <Button key={s} size="sm" variant="primary" onClick={() => (props.run(() => props.api.fit(s, item.part.uid)), setPicked(null))}>
              Fit: {props.screen.slots.find((x) => x.id === s)?.name ?? s}
            </Button>
          ))}
          {item.stand_fits.map((f) => (
            <Button
              key={`${f.assembly}:${f.slot}`}
              size="sm"
              onClick={() => (props.run(() => props.api.build(f.assembly, f.slot, item.part.uid)), setPicked(null))}
            >
              Fit on stand: {props.screen.stands.find((s) => s.uid === f.assembly)?.slots.find((x) => x.id === f.slot)?.name ?? f.slot}
            </Button>
          ))}
          {item.reason && <span class="wp-warn">{item.reason}</span>}
          <Button size="sm" variant="danger" onClick={() => (props.run(() => props.api.sell(item.part.uid)), setPicked(null))}>
            Sell <Money value={item.sale_price} />
          </Button>
        </div>
      )}
    </div>
  );
}

function WorkshopScreenView(props: { api: WorkshopApi; host: WorkshopHost; state: WorkshopState; initial: WorkshopScreen }) {
  const { api, host } = props;
  const state = useWorkshopState(props.state);
  const [screen, setScreen] = useState(props.initial);
  const [error, setError] = useState<string | null>(null);
  const [catalog, setCatalog] = useState<{ kind: string | null } | null>(null);
  const [list, setList] = useState(false);
  const [stand, setStand] = useState(false);
  const [lift, setLift] = useState(false);
  const run = (f: () => WorkshopScreen) => {
    try {
      const next = f();
      setScreen(next);
      setError(null);
      host.changed(next);
    } catch (e) {
      setError(String((e as Error).message ?? e).replace(/^(Error: )?wp_wasm: /, ""));
    }
  };
  const back = useRef(host.back);
  back.current = host.back;
  const modal = useRef(false);
  modal.current = !!catalog || stand;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      if (modal.current) (setCatalog(null), setStand(false));
      else back.current();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  const slot = screen.slots.find((s) => s.id === state.selected) ?? null;
  const slotName = (id: string) => screen.slots.find((s) => s.id === id)?.name ?? id;
  const hover = state.hover ? screen.slots.find((s) => s.id === state.hover) : null;
  const cam = (p: WorkshopCam, label: string) => (
    <Button size="sm" onClick={() => host.camera(p)}>
      {label}
    </Button>
  );
  return (
    <div class="wp-ws" data-testid="workshop">
      <header class="wp-ws-top">
        <h1>
          <small>// WORKSHOP</small>
          {host.carName}
        </h1>
        <div class="wp-row" style={{ gap: "0.8em", flexWrap: "wrap" }}>
          <span>
            CASH <Money value={screen.cash} tone />
          </span>
          {screen.torque_factor < 0.995 && (
            <Badge tone="warn" title="worn engine parts: replace them to get the power back">
              engine worn: −{Math.round((1 - screen.torque_factor) * 100)}% torque
            </Badge>
          )}
          {screen.missing.length > 0 ? (
            <Badge tone="danger" title={screen.missing.join(", ")}>
              missing: {screen.missing.length > 3 ? `${screen.missing.slice(0, 3).join(", ")}…` : screen.missing.join(", ")}
            </Badge>
          ) : (
            <Badge tone="ok">ready to drive</Badge>
          )}
          <Button size="sm" onClick={host.back} hotkey="Esc">
            Garage
          </Button>
          <Button size="sm" variant="primary" disabled={screen.missing.length > 0} onClick={host.drive}>
            Drive
          </Button>
        </div>
      </header>
      <nav class="wp-ws-tools" aria-label="workshop tools">
        <div class="wp-dim wp-mono wp-upper">View</div>
        {cam("overview", "Overview")}
        {cam("engine", "Engine bay")}
        {cam("under", "Underside")}
        {cam("wheels", "Wheels")}
        <div class="wp-dim wp-mono wp-upper">Lift</div>
        <Button size="sm" pressed={lift} onClick={() => (host.lift(!lift), setLift(!lift))}>
          {lift ? "Lower car" : "Raise car"}
        </Button>
        <div class="wp-dim wp-mono wp-upper">Parts</div>
        <Button size="sm" onClick={() => setCatalog({ kind: null })}>
          Catalog
        </Button>
        <Button size="sm" pressed={list} onClick={() => setList(!list)}>
          Part list
        </Button>
        <Button size="sm" pressed={stand} onClick={() => setStand(!stand)} title="strip and build a loose engine">
          Engine stand{screen.stands.length > 0 ? ` (${screen.stands.length})` : ""}
        </Button>
      </nav>
      {list && <PartsList screen={screen} state={state} onClose={() => setList(false)} />}
      {hover && hover.id !== state.selected && (
        <div class="wp-ws-hover wp-mono">
          {hover.name}
          {hover.fitted ? ` · ${pct(hover.fitted.condition)}` : " · empty"}
        </div>
      )}
      <aside class="wp-ws-right">
        {error && (
          <p class="wp-danger" data-testid="ws-error">
            {error}
          </p>
        )}
        {slot ? (
          <PartCard slot={slot} screen={screen} run={run} api={api} slotName={slotName} openCatalog={(kind) => setCatalog({ kind })} />
        ) : (
          <Panel title="Workshop">
            <p class="wp-dim">Click a part on the car to work on it. Drag to look around, scroll to zoom.</p>
            {screen.upgrades.length > 0 && <p class="wp-dim">Other upgrades: {screen.upgrades.join(", ")}</p>}
          </Panel>
        )}
      </aside>
      <InventoryStrip screen={screen} run={run} api={api} host={host} state={state} />
      {catalog && <Catalog screen={screen} kind={catalog.kind} run={run} api={api} onClose={() => setCatalog(null)} />}
      {stand && !catalog && <EngineStand screen={screen} run={run} api={api} onClose={() => setStand(false)} />}
    </div>
  );
}

/** Mounts the workshop overlay into `el` (full-screen; only its panels take the mouse). */
export class WorkshopOverlay {
  open = false;
  readonly state = new WorkshopState();
  private key = 0;

  constructor(private readonly el: HTMLElement) {}

  show(api: WorkshopApi, host: WorkshopHost): void {
    this.open = true;
    this.key++;
    this.state.set({ selected: null, hover: null });
    render(
      <div class="wp-ui wp-root" style={{ position: "absolute", inset: 0, pointerEvents: "none", zIndex: 80 }}>
        <WorkshopScreenView key={this.key} api={api} host={host} state={this.state} initial={api.screen()} />
      </div>,
      this.el,
    );
  }

  hide(): void {
    this.open = false;
    render(null, this.el);
  }
}
