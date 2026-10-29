/**
 * Garage workshop overlay: the screen around the 3D bay (the host draws the car and the room;
 * see apps/testbed-web/src/workshop). Patterned on a hands-on car-building garage: the car is the
 * screen, panels sit at the edges and the middle stays clear for the mouse.
 *
 * - Top bar: car, cash, what is missing before the car can drive, back to the garage, drive.
 * - Left toolbar: camera views (overview, engine bay, underside, wheels), the lift, the parts
 *   catalog and the car's part list.
 * - Right: the part card of the selected slot. Do it yourself (take off what is in the way in
 *   one click, remove, fit from the inventory) or pay the mechanic to swap the part (from the
 *   inventory or bought new; everything around it goes back on). Old parts are kept or sold.
 *   The card lists the slot's upgrades with prices (part grades and bolt-on upgrades of its
 *   group): this is where mods are bought. One that needs another part first offers both.
 * - Bottom: the inventory strip; choosing a part lights up where it fits.
 * - Engine stand: a loose engine (taken out whole) stripped and built part by part, in order.
 * - Paint shop: respray, vinyls, finishes and cosmetic parts, previewed on the car (PaintShop).
 *
 * Every action goes through `WorkshopApi` and redraws from the screen it returns; the overlay
 * holds no game state of its own.
 */
import { render } from "preact";
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { Badge, Button, Confirm, isBigSpend, Meter, Money, Panel, severityColor, Tabs } from "../ui/kit";
import { PaintShop } from "./PaintShop";
import type {
  CatalogEntry,
  InventoryView,
  MechanicResult,
  MechanicSource,
  ModOption,
  ModTreeView,
  PartGroup,
  SlotView,
  StandView,
  WorkshopApi,
  WorkshopScreen,
} from "./types";
import { boltOns, missingFor, onCar, ownedState, plan, planNames, upgradeGroup, type Plan } from "./upgrades";
import "./garage.css";

/** Camera presets the host knows. */
export type WorkshopCam = "overview" | "engine" | "rear" | "under" | "wheels" | "stand";

/** What the host (3D side) does for the overlay. */
export interface WorkshopHost {
  /** Highlight these slots on the car (empty = none). */
  highlight(slots: string[]): void;
  /** Highlight these parts of the engine on the stand (empty = none). */
  highlightStand(slots: string[]): void;
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

/** A workshop preference kept in the browser (sort orders, keep/sell old parts). */
function usePref<T extends string>(key: string, fallback: T, allowed: readonly T[]): [T, (v: T) => void] {
  const [v, setV] = useState<T>(() => {
    try {
      const got = localStorage.getItem(`wp.workshop.${key}`) as T | null;
      return got && allowed.includes(got) ? got : fallback;
    } catch {
      return fallback;
    }
  });
  const set = (next: T) => {
    setV(next);
    try {
      localStorage.setItem(`wp.workshop.${key}`, next);
    } catch {
      // Storage blocked: the choice lasts until the page closes.
    }
  };
  return [v, set];
}

/** A small "Sort: ..." dropdown. */
function SortSelect<T extends string>(props: { value: T; options: { id: T; label: string }[]; onChange: (v: T) => void; testid: string }) {
  return (
    <label class="wp-ws-sort wp-dim wp-mono">
      SORT
      <select
        data-testid={props.testid}
        value={props.value}
        onChange={(e) => props.onChange((e.currentTarget as HTMLSelectElement).value as T)}
      >
        {props.options.map((o) => (
          <option key={o.id} value={o.id}>
            {o.label}
          </option>
        ))}
      </select>
    </label>
  );
}

const OLD_PARTS = ["keep", "sell"] as const;
type OldParts = (typeof OLD_PARTS)[number];

/** What the mechanic did, in one line. */
function jobLine(what: string, r: MechanicResult): string {
  const j = r.job;
  const old = j.sold > 0 ? `, old part sold for $${j.sold.toLocaleString()}` : j.kept.length > 0 ? `, old part${j.kept.length > 1 ? "s" : ""} in the inventory` : "";
  const bought = j.bought > 0 ? `parts $${j.bought.toLocaleString()} + ` : "";
  return `Mechanic fitted ${what}: ${bought}labour $${j.fee.toLocaleString()} (${j.touched} part${j.touched > 1 ? "s" : ""} handled)${old}.`;
}

/** Shared state between the 3D side and the overlay (selection, hover). */
export class WorkshopState {
  selected: string | null = null;
  hover: string | null = null;
  /** Part of the engine on the stand picked in 3D, or under the mouse. */
  standSelected: string | null = null;
  standHover: string | null = null;
  private listeners = new Set<() => void>();
  set(patch: Partial<Pick<WorkshopState, "selected" | "hover" | "standSelected" | "standHover">>): void {
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

/** Runs a mechanic job and reports it. */
type Hire = (slot: string, source: MechanicSource, what: string) => void;

function PartCard(props: {
  slot: SlotView;
  screen: WorkshopScreen;
  run: (f: () => WorkshopScreen) => void;
  hire: Hire;
  api: WorkshopApi;
  slotName: (id: string) => string;
  openCatalog: (kind: string) => void;
  oldParts: OldParts;
  setOldParts: (v: OldParts) => void;
  shop: Shop;
}) {
  const { slot, screen, run, api, hire } = props;
  const f = slot.fitted;
  const fits = screen.inventory.filter((p) => p.fits.includes(slot.id));
  const stock = screen.catalog.find((c) => c.kind === slot.kind && c.grade === null);
  const way = slot.in_the_way;
  const fee = slot.mechanic_fee;
  const wayNames = way.map(props.slotName).join(", ");
  return (
    <Panel
      title={slot.name}
      tag={
        <span class="wp-dim">
          {slot.parent ? `inside ${props.slotName(slot.parent).toLowerCase()}` : slot.mount ? `on the ${props.slotName(slot.mount).toLowerCase()}` : slot.group}
        </span>
      }
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
          </>
        ) : (
          <div class="wp-row" style={{ justifyContent: "space-between" }}>
            <span class="wp-dim">Empty</span>
            {slot.required && <Badge tone="danger">needed to drive</Badge>}
          </div>
        )}

        <div class="wp-ws-section wp-dim wp-mono wp-upper">Do it yourself</div>
        {way.length > 0 && (
          <Button
            size="sm"
            testid="ws-clear-way"
            title="take every part in the way off into the inventory"
            onClick={() => run(() => api.clearWay(slot.id))}
          >
            Take off what's in the way ({wayNames})
          </Button>
        )}
        {f ? (
          <>
            <Button
              variant="primary"
              disabled={!!slot.remove_blocker}
              title={slot.remove_blocker ?? "take it off into the inventory"}
              onClick={() => run(() => api.remove(slot.id))}
            >
              Remove
            </Button>
            {!slot.remove_blocker && (slot.takes_along?.length ?? 0) > 0 && (
              <span class="wp-dim" data-testid="ws-takes-along">
                comes off with it: {slot.takes_along!.map(props.slotName).join(", ")}
              </span>
            )}
          </>
        ) : fits.length > 0 ? (
          fits.map((p) => (
            <Button key={p.part.uid} disabled={way.length > 0} onClick={() => run(() => api.fit(slot.id, p.part.uid))} title="fit this part">
              Fit {p.name} ({pct(p.part.condition)})
            </Button>
          ))
        ) : (
          <span class="wp-dim">No part in the inventory fits here.</span>
        )}

        <div class="wp-ws-section wp-dim wp-mono wp-upper">
          Mechanic <span style={{ textTransform: "none" }}>· labour <Money value={fee} /></span>
        </div>
        <div class="wp-ws-mech" data-testid="ws-mechanic">
          {fits.map((p) => (
            <Button
              key={p.part.uid}
              size="sm"
              variant="primary"
              disabled={fee > screen.cash}
              title={way.length ? `takes off and refits: ${wayNames}` : "swap the part for you"}
              onClick={() => hire(slot.id, { part: p.part.uid }, p.name)}
            >
              Install {p.name} ({pct(p.part.condition)}) <Money value={fee} />
            </Button>
          ))}
          {stock && (
            <Button
              size="sm"
              variant={fits.length ? undefined : "primary"}
              testid="ws-mech-stock"
              disabled={!!stock.reason || stock.price + fee > screen.cash}
              title={stock.reason ?? "a new stock part, fitted"}
              onClick={() => hire(slot.id, { buy: null }, `a new stock ${slot.name.toLowerCase()}`)}
            >
              New stock part <Money value={stock.price + fee} />
            </Button>
          )}
        </div>
        {f && (
          <div class="wp-row wp-ws-oldparts" style={{ gap: "0.4em" }}>
            <span class="wp-dim">Old part:</span>
            {OLD_PARTS.map((o) => (
              <Button key={o} size="sm" pressed={props.oldParts === o} testid={`ws-old-${o}`} onClick={() => props.setOldParts(o)}>
                {o === "keep" ? "Keep it" : "Sell it"}
              </Button>
            ))}
          </div>
        )}
        {!f && stock && (
          <Button size="sm" variant="ghost" disabled={stock.price > screen.cash} onClick={() => run(() => api.buy(stock.kind, null))} title="a new stock part into the inventory, to fit yourself">
            Buy stock into the inventory <Money value={stock.price} />
          </Button>
        )}

        <div class="wp-ws-section wp-dim wp-mono wp-upper">
          Upgrades <span style={{ textTransform: "none" }}>· buy here, fit yourself or have them fitted</span>
        </div>
        <SlotUpgrades shop={props.shop} slot={slot} />
        <Button size="sm" variant="ghost" onClick={() => props.openCatalog(slot.kind)} title="every part and upgrade for this slot's group">
          Full catalog
        </Button>
      </div>
    </Panel>
  );
}

/** Buying upgrades: the actions every upgrade row (part card, catalog) shares. */
interface Shop {
  screen: WorkshopScreen;
  /** The car's mod tree (requirements, bolt-on upgrades); null when it cannot be read. */
  tree: ModTreeView | null;
  /** Slot on the car the mechanic fits a part of `kind` in: `prefer` if it takes one, else the
   * first that does not already carry `grade`, else the first. */
  target(kind: string, prefer?: string | null, grade?: string | null): SlotView | null;
  /** Buy a part into the inventory. */
  buy(c: CatalogEntry): void;
  /** Buy a part and have the mechanic fit it in `slot`. */
  fit(c: CatalogEntry, slot: SlotView): void;
  /** Have the mechanic fit inventory part `part` in `slot` (labour only). */
  fitOwned(part: InventoryView, slot: SlotView): void;
  /** Put a bolt-on upgrade on the car / take it off (40 % back). */
  install(m: ModOption): void;
  uninstall(m: ModOption): void;
  /** Buy a mod with everything it needs first; `fit` = the mechanic fits the parts too. */
  buyPlan(p: Plan, fit: boolean, prefer?: string | null): void;
}

/**
 * One upgrade for sale: a part grade (`entry`) or a bolt-on upgrade (`entry` null). A part
 * that needs another part on first stays clickable and offers both together. What the player
 * already has (`ownedState`) is greyed out with its buy buttons disabled: a part on the car, a
 * bolt-on installed (Remove stays), a part in the inventory (no Buy; the mechanic can still fit
 * that part). Buying or fitting keeps the window it is in open (owner, 2026-10-29).
 */
function UpgradeRow(props: { shop: Shop; entry: CatalogEntry | null; mod: ModOption | null; prefer?: string | null }) {
  const { shop, entry, mod } = props;
  const cash = shop.screen.cash;
  const [ask, setAsk] = useState<{ text: string; price: number; go: () => void } | null>(null);
  /** Buying, fitting and installing never ask (owner, 2026-10-27). */
  const spend = (_text: string, _price: number, go: () => void) => go();
  /** Only a removal that throws away a lot of money waits for "are you sure?". */
  const loss = (text: string, price: number, go: () => void) => (isBigSpend(price, cash) ? setAsk({ text, price, go }) : go());
  const name = entry?.name ?? mod?.name ?? "?";
  const missing = mod && shop.tree ? missingFor(shop.tree, mod.id) : [];
  const p = missing.length && mod && shop.tree ? plan(shop.screen, shop.tree, mod.id) : null;
  // Fitting by hand needs every bolt-on in the chain to sit on a fitted part: a bolt-on after a
  // part bought into the inventory could not go on yet.
  const canBuyLoose = !!p && !p.steps.some((s, i) => !s.entry && p.steps.slice(0, i).some((t) => t.entry && !t.owned));
  const slotsFor = p ? p.steps.filter((s) => s.entry).map((s) => shop.target(s.entry!.kind, props.prefer)) : [];
  const noSlot = slotsFor.some((s) => !s);
  const t = entry ? shop.target(entry.kind, props.prefer, entry.grade) : null;
  const installed = mod?.installed ?? false;
  const own = ownedState(shop.screen, entry, mod, props.prefer);
  /** Greyed out: the player has it on the car (an inventory part can still be fitted). */
  const greyed = own?.state === "fitted" || own?.state === "installed";
  let buttons;
  if (entry && own?.state === "fitted") {
    buttons = (
      <>
        <Button size="sm" testid="ws-up-buy" disabled title={own.title}>
          Buy
        </Button>
        <Button size="sm" testid="ws-cat-mechanic" disabled title={own.title}>
          Fit for me <Money value={entry.price + entry.install_fee} />
        </Button>
      </>
    );
  } else if (entry && own?.state === "inventory" && own.part) {
    const part = own.part;
    const slot = t;
    buttons = (
      <>
        <Button size="sm" testid="ws-up-buy" disabled title={own.title}>
          Buy
        </Button>
        <Button
          size="sm"
          variant="primary"
          testid="ws-cat-mechanic"
          disabled={!slot || entry.install_fee > shop.screen.cash}
          title={slot ? `the mechanic fits the one in your inventory: labour $${entry.install_fee.toLocaleString()}` : "the part it goes in is not on the car"}
          onClick={() => slot && shop.fitOwned(part, slot)}
        >
          Fit mine for me <Money value={entry.install_fee} />
        </Button>
      </>
    );
  } else if (p) {
    const both = p.steps.length > 2 ? `Buy all ${p.steps.length}` : "Buy both";
    const blocked = p.blocked ?? (noSlot ? "a part it needs has no slot on the car" : null);
    buttons = (
      <>
        {entry && canBuyLoose && p.price > 0 && (
          <Button
            size="sm"
            variant="primary"
            testid="ws-buy-both"
            disabled={!!blocked || p.price > cash}
            title={blocked ?? `into the inventory: ${planNames(p)}`}
            onClick={() => spend(`Buy ${planNames(p)}`, p.price, () => shop.buyPlan(p, false, props.prefer))}
          >
            {both} <Money value={p.price} />
          </Button>
        )}
        <Button
          size="sm"
          variant={entry && canBuyLoose && p.price > 0 ? undefined : "primary"}
          testid="ws-fit-both"
          disabled={!!blocked || p.price + p.labour > cash}
          title={blocked ?? `bought and fitted in order: ${planNames(p)}${p.labour ? `; labour $${p.labour.toLocaleString()}` : ""}`}
          onClick={() => spend(`Buy and fit ${planNames(p)}`, p.price + p.labour, () => shop.buyPlan(p, true, props.prefer))}
        >
          {entry && canBuyLoose ? (p.steps.length > 2 ? `Fit all ${p.steps.length} for me` : "Fit both for me") : p.labour ? `${both}, fitted` : both}{" "}
          <Money value={p.price + p.labour} />
        </Button>
      </>
    );
  } else if (entry) {
    buttons = (
      <>
        <Button
          size="sm"
          variant="primary"
          testid="ws-up-buy"
          disabled={!!entry.reason || entry.price > cash}
          title={entry.reason ?? "into the inventory, to fit yourself"}
          onClick={() => spend(`Buy ${name}`, entry.price, () => shop.buy(entry))}
        >
          Buy
        </Button>
        <Button
          size="sm"
          testid="ws-cat-mechanic"
          disabled={!t || !!entry.reason || entry.price + entry.install_fee > cash}
          title={entry.reason ?? (t ? `bought and fitted by the mechanic: labour $${entry.install_fee.toLocaleString()}` : "the part it goes in is not on the car")}
          onClick={() => t && spend(`Buy and have the mechanic fit ${name}`, entry.price + entry.install_fee, () => shop.fit(entry, t))}
        >
          Fit for me <Money value={entry.price + entry.install_fee} />
        </Button>
      </>
    );
  } else if (mod && installed) {
    buttons = (
      <Button
        size="sm"
        testid="ws-up-remove"
        disabled={!mod.allowed}
        title={mod.reason ?? `take it off: $${Math.round(mod.price * 0.4).toLocaleString()} back`}
        onClick={() => loss(`Remove ${name} (40 % back)`, mod.price * 0.6, () => shop.uninstall(mod))}
      >
        Remove
      </Button>
    );
  } else if (mod) {
    buttons = (
      <Button
        size="sm"
        variant="primary"
        testid="ws-up-install"
        disabled={!mod.allowed}
        title={mod.reason ?? (mod.replaces ? `replaces ${mod.replaces} (40 % back)` : "put it on the car")}
        onClick={() => spend(`${mod.replaces ? "Swap in" : "Install"} ${name}`, mod.price, () => shop.install(mod))}
      >
        {mod.replaces ? "Swap" : "Install"}
      </Button>
    );
  }
  return (
    <div
      class={`wp-ws-uprow${greyed ? " wp-ws-owned" : ""}`}
      data-testid="ws-catrow"
      data-kind={entry?.kind ?? ""}
      data-grade={entry?.grade ?? mod?.id ?? ""}
      data-owned={own?.state ?? ""}
    >
      <div class="wp-ws-catrow">
        <span class={greyed ? "wp-dim" : undefined}>
          {name}
          {entry && entry.count > 1 && <span class="wp-dim"> ×{entry.count}</span>}
          {(entry?.tier ?? mod?.tier ?? 0) > 0 && <span class="wp-dim"> · T{entry?.tier ?? mod?.tier}</span>}
          {entry?.replaces && entry.replaces.length > 0 && (
            <span class="wp-dim" data-testid="ws-cat-replaces"> · replaces {entry.replaces.join(", ")} (refunded at 40 %)</span>
          )}
          {own && (
            <>
              {" "}
              <Badge tone={own.state === "inventory" ? "info" : "ok"} title={own.title}>
                <span data-testid="ws-owned">{own.label}</span>
              </Badge>
            </>
          )}
        </span>
        <span class={greyed ? "wp-dim" : undefined}>
          <Money value={entry?.price ?? mod?.price ?? 0} />
        </span>
      </div>
      <div class="wp-row wp-ws-upbtns">{buttons}</div>
      {missing.length > 0 && (
        <div class="wp-warn wp-ws-upnote" data-testid="ws-needs">
          needs {missing.map((m) => m.name).join(" and ")} first
        </div>
      )}
      {mod?.tradeoff && <div class="wp-dim wp-ws-upnote">{mod.tradeoff}</div>}
      {ask && (
        <Confirm
          testid="ws-confirm"
          text={
            <>
              {ask.text} for <Money value={ask.price} />? You have <Money value={cash} />.
            </>
          }
          yes="Remove"
          onYes={() => {
            const go = ask.go;
            setAsk(null);
            go();
          }}
          onNo={() => setAsk(null)}
        />
      )}
    </div>
  );
}

/** Upgrades for sale for one slot: its part grades, then the bolt-ons of its group. */
function SlotUpgrades(props: { shop: Shop; slot: SlotView }) {
  const { shop, slot } = props;
  const grades = shop.screen.catalog.filter((c) => c.kind === slot.kind && c.grade !== null).sort((a, b) => a.tier - b.tier || a.price - b.price);
  const bolts = shop.tree ? boltOns(shop.tree, shop.screen.catalog).filter((m) => upgradeGroup(m) === slot.group) : [];
  if (!grades.length && !bolts.length) return <span class="wp-dim">No upgrades for this part.</span>;
  const modOf = (id: string | null) => shop.tree?.mods.find((m) => m.id === id) ?? null;
  return (
    <div class="wp-ws-upgrades" data-testid="ws-upgrades">
      {grades.map((c) => (
        <UpgradeRow key={c.grade} shop={shop} entry={c} mod={modOf(c.grade)} prefer={slot.id} />
      ))}
      {bolts.length > 0 && <div class="wp-dim wp-mono wp-upper wp-ws-upsub">Bolt-on · {slot.group}</div>}
      {bolts.map((m) => (
        <UpgradeRow key={m.id} shop={shop} entry={null} mod={m} prefer={slot.id} />
      ))}
    </div>
  );
}

const CATALOG_SORTS = ["tier", "price", "name"] as const;
type CatalogSort = (typeof CATALOG_SORTS)[number];

function Catalog(props: {
  shop: Shop;
  kind: string | null;
  /** Slot the catalog was opened for (the mechanic fits there). */
  slot: string | null;
  /** What the last purchase did / why it failed: shown in the window, which stays open. */
  notice: string | null;
  error: string | null;
  onClose: () => void;
}) {
  const { shop } = props;
  const [sort, setSort] = usePref<CatalogSort>("catalogSort", "tier", CATALOG_SORTS);
  const bolts = shop.tree ? boltOns(shop.tree, shop.screen.catalog) : [];
  const groups = GROUPS.filter((g) => shop.screen.catalog.some((c) => c.group === g) || bolts.some((m) => upgradeGroup(m) === g));
  const first = props.kind ? shop.screen.catalog.find((c) => c.kind === props.kind)?.group : undefined;
  const [group, setGroup] = useState<PartGroup>(first ?? groups[0] ?? "Body");
  const [kind, setKind] = useState<string | null>(props.kind);
  const rows = shop.screen.catalog.filter((c) => c.group === group && (!kind || c.kind === kind));
  const byKind = new Map<string, CatalogEntry[]>();
  for (const r of rows) byKind.set(r.kind, [...(byKind.get(r.kind) ?? []), r]);
  const order = (a: { name: string; price: number; tier: number }, b: { name: string; price: number; tier: number }) =>
    sort === "price" ? a.price - b.price : sort === "name" ? a.name.localeCompare(b.name) : a.tier - b.tier || a.price - b.price;
  for (const list of byKind.values()) list.sort(order);
  const groupBolts = kind ? [] : bolts.filter((m) => upgradeGroup(m) === group).sort(order);
  const modOf = (id: string | null) => shop.tree?.mods.find((m) => m.id === id) ?? null;
  return (
    <div class="wp-ws-modal" data-testid="ws-catalog">
      <Panel
        title="Parts catalog"
        tag={
          <span class="wp-row" style={{ gap: "0.6em" }}>
            <Money value={shop.screen.cash} tone />
            <Button size="sm" onClick={props.onClose} hotkey="Esc">
              Close
            </Button>
          </span>
        }
      >
        {props.error && (
          <p class="wp-danger wp-ws-notice" data-testid="ws-cat-error">
            {props.error}
          </p>
        )}
        {props.notice && (
          <p class="wp-ok wp-ws-notice" data-testid="ws-cat-notice">
            {props.notice}
          </p>
        )}
        <Tabs tabs={groups.map((g) => ({ id: g, label: g }))} value={group} onChange={(g) => (setGroup(g), setKind(null))} />
        <div class="wp-row" style={{ margin: "0.4em 0", justifyContent: "flex-end" }}>
          <SortSelect
            testid="ws-catalog-sort"
            value={sort}
            onChange={setSort}
            options={[
              { id: "tier", label: "Tier" },
              { id: "price", label: "Price" },
              { id: "name", label: "Name" },
            ]}
          />
        </div>
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
                <UpgradeRow key={c.grade ?? "stock"} shop={shop} entry={c} mod={modOf(c.grade)} prefer={props.slot} />
              ))}
            </div>
          ))}
          {groupBolts.length > 0 && (
            <div class="wp-ws-catgroup">
              <div class="wp-dim wp-mono wp-upper">Bolt-on upgrades</div>
              {groupBolts.map((m) => (
                <UpgradeRow key={m.id} shop={shop} entry={null} mod={m} prefer={props.slot} />
              ))}
            </div>
          )}
        </div>
      </Panel>
    </div>
  );
}

const LIST_SORTS = ["group", "name", "condition"] as const;
type ListSort = (typeof LIST_SORTS)[number];

/** Condition for sorting: empty required slots first, empty optional ones last. */
const slotCondition = (s: SlotView) => (s.fitted ? s.fitted.condition : s.required ? -1 : 2);

function PartsList(props: { screen: WorkshopScreen; state: WorkshopState; onClose: () => void }) {
  const [sort, setSort] = usePref<ListSort>("listSort", "group", LIST_SORTS);
  const row = (s: SlotView, indent: boolean) => (
    <button
      type="button"
      key={s.id}
      class="wp-ws-slot"
      data-testid="ws-slotrow"
      data-slot={s.id}
      style={indent && s.parent ? { paddingLeft: "1.4em" } : undefined}
      aria-selected={props.state.selected === s.id}
      onClick={() => props.state.set({ selected: s.id })}
    >
      <span>{s.name}</span>
      <span class="wp-mono" style={{ color: s.fitted ? severityColor(1 - s.fitted.condition) : "var(--dim)" }}>
        {s.fitted ? pct(s.fitted.condition) : s.required ? "MISSING" : "—"}
      </span>
    </button>
  );
  const slots = onCar(props.screen);
  const flat =
    sort === "name"
      ? [...slots].sort((a, b) => a.name.localeCompare(b.name))
      : [...slots].sort((a, b) => slotCondition(a) - slotCondition(b) || a.name.localeCompare(b.name));
  return (
    <div class="wp-ws-list" data-testid="ws-partlist">
      <Panel title="Car parts" tag={<Button size="sm" onClick={props.onClose}>Close</Button>}>
        <div class="wp-row" style={{ marginBottom: "0.4em", justifyContent: "flex-end" }}>
          <SortSelect
            testid="ws-list-sort"
            value={sort}
            onChange={setSort}
            options={[
              { id: "group", label: "Group" },
              { id: "name", label: "Name" },
              { id: "condition", label: "Worst first" },
            ]}
          />
        </div>
        {sort === "group"
          ? GROUPS.map((g) => {
              const inGroup = slots.filter((s) => s.group === g);
              if (!inGroup.length) return null;
              return (
                <div key={g} style={{ marginBottom: "0.5em" }}>
                  <div class="wp-dim wp-mono wp-upper">{g}</div>
                  {inGroup.map((s) => row(s, true))}
                </div>
              );
            })
          : flat.map((s) => row(s, false))}
      </Panel>
    </div>
  );
}

/** One loose engine on the stand: its internals in order, strip and build. */
function Stand(props: {
  stand: StandView;
  screen: WorkshopScreen;
  run: (f: () => WorkshopScreen) => void;
  api: WorkshopApi;
  state: WorkshopState;
  host: WorkshopHost;
}) {
  const { stand, screen, run, api, state, host } = props;
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
          <div
            key={s.id}
            class="wp-ws-standrow"
            data-testid="ws-stand-slot"
            data-slot={s.id}
            aria-selected={state.standSelected === s.id}
            ref={(el) => {
              if (state.standSelected === s.id) el?.scrollIntoView({ block: "nearest" });
            }}
            onMouseEnter={() => host.highlightStand([s.id])}
            onMouseLeave={() => host.highlightStand(state.standSelected ? [state.standSelected] : [])}
            onClick={() => state.set({ standSelected: s.id })}
          >
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
                  <Button
                    key={p.part.uid}
                    size="sm"
                    variant="primary"
                    title={`fit ${p.name}`}
                    onClick={() => run(() => api.build(stand.uid, s.id, p.part.uid))}
                  >
                    Fit {p.part.grade ? p.name : ""} {pct(p.part.condition)}
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

function EngineStand(props: {
  screen: WorkshopScreen;
  run: (f: () => WorkshopScreen) => void;
  api: WorkshopApi;
  state: WorkshopState;
  host: WorkshopHost;
  onClose: () => void;
}) {
  return (
    <div class="wp-ws-standpanel" data-testid="ws-engine-stand">
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
          props.screen.stands.map((st) => (
            <Stand key={st.uid} stand={st} screen={props.screen} run={props.run} api={props.api} state={props.state} host={props.host} />
          ))
        )}
        <p class="wp-dim">Parts come out top down: manifolds, valve cover, timing, cams, valves, head, gasket, pistons, rods, crank.</p>
      </Panel>
    </div>
  );
}

const INVENTORY_SORTS = ["recent", "name", "condition", "value", "fits"] as const;
type InventorySort = (typeof INVENTORY_SORTS)[number];

function InventoryStrip(props: {
  screen: WorkshopScreen;
  run: (f: () => WorkshopScreen) => void;
  api: WorkshopApi;
  host: WorkshopHost;
  state: WorkshopState;
}) {
  const [picked, setPicked] = useState<number | null>(null);
  const [sort, setSort] = usePref<InventorySort>("inventorySort", "recent", INVENTORY_SORTS);
  const item = props.screen.inventory.find((p) => p.part.uid === picked) ?? null;
  const fitsHere = (p: InventoryView) => (p.fits.length > 0 ? 0 : p.stand_fits.length > 0 ? 1 : 2);
  const items = [...props.screen.inventory];
  if (sort === "recent") items.reverse();
  else
    items.sort((a, b) =>
      sort === "name"
        ? a.name.localeCompare(b.name)
        : sort === "condition"
          ? a.part.condition - b.part.condition
          : sort === "value"
            ? b.sale_price - a.sale_price
            : fitsHere(a) - fitsHere(b) || a.name.localeCompare(b.name),
    );
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
      {/* Parts bolted onto it travel with it (an engine's internals show on the stand). */}
      {!props.screen.stands.some((s) => s.uid === p.part.uid) && (p.part.contents?.length ?? 0) > 0 && (
        <span class="wp-dim wp-mono">+ {p.part.contents!.map((c) => c.kind.replace(/_/g, " ")).join(", ")}</span>
      )}
      {conditionBar(p.part.condition)}
    </button>
  );
  return (
    <div class="wp-ws-strip">
      <div class="wp-row" style={{ justifyContent: "space-between" }}>
        <span class="wp-dim wp-mono wp-upper">Inventory ({props.screen.inventory.length})</span>
        <SortSelect
          testid="ws-inventory-sort"
          value={sort}
          onChange={setSort}
          options={[
            { id: "recent", label: "Newest" },
            { id: "name", label: "Name" },
            { id: "condition", label: "Worst first" },
            { id: "value", label: "Value" },
            { id: "fits", label: "Fits this car" },
          ]}
        />
      </div>
      <div class="wp-ws-tiles" data-testid="ws-inventory">
        {items.length === 0 ? <span class="wp-dim">Inventory empty: parts you take off or buy land here.</span> : items.map(tile)}
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
  const [catalog, setCatalog] = useState<{ kind: string | null; slot: string | null } | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  /** The catalog opens without the last job's line (it shows what is bought from it). */
  const openCatalog = (c: { kind: string | null; slot: string | null }) => (setNotice(null), setCatalog(c));
  const [oldParts, setOldParts] = usePref<OldParts>("oldParts", "keep", OLD_PARTS);
  const [list, setList] = useState(false);
  const [stand, setStand] = useState(false);
  const [lift, setLift] = useState(false);
  const [paint, setPaint] = useState(false);
  const act = (f: () => { screen: WorkshopScreen; notice: string | null }) => {
    try {
      const next = f();
      setScreen(next.screen);
      setError(null);
      setNotice(next.notice);
      host.changed(next.screen);
    } catch (e) {
      setError(String((e as Error).message ?? e).replace(/^(Error: )?wp_wasm: /, ""));
    }
  };
  const run = (f: () => WorkshopScreen) => act(() => ({ screen: f(), notice: null }));
  const hire: Hire = (slotId, source, what) =>
    act(() => {
      const r = api.mechanic(slotId, source, oldParts === "sell");
      return { screen: r.workshop, notice: jobLine(what, r) };
    });
  // The mod tree is read again after every change (cash, what is installed).
  const tree = useMemo(() => {
    try {
      return api.mods?.() ?? null;
    } catch {
      return null;
    }
  }, [screen]);
  const onCarSlots = onCar(screen);
  const target = (kind: string, prefer?: string | null, grade?: string | null) =>
    onCarSlots.find((s) => s.id === prefer && s.kind === kind) ??
    (grade ? onCarSlots.find((s) => s.kind === kind && s.fitted?.grade !== grade) : undefined) ??
    onCarSlots.find((s) => s.kind === kind) ??
    null;
  const shop: Shop = {
    screen,
    tree,
    target,
    buy: (c) => act(() => ({ screen: api.buy(c.kind, c.grade), notice: `Bought ${c.count > 1 ? `${c.name} ×${c.count}` : c.name}: in the inventory.` })),
    fit: (c, slot) => hire(slot.id, { buy: c.grade }, c.count > 1 ? `${c.name} ×${c.count}` : c.name),
    fitOwned: (part, slot) => hire(slot.id, { part: part.part.uid }, part.name),
    install: (m) => api.install && act(() => ({ screen: api.install!(m.id), notice: `Installed ${m.name}.` })),
    uninstall: (m) => api.uninstall && act(() => ({ screen: api.uninstall!(m.id), notice: `Removed ${m.name}.` })),
    buyPlan: (p, fit, prefer) => {
      // Step by step, in order: each step is its own purchase, so a refusal halfway keeps what
      // was already bought (the inventory has it) and says so.
      const done: string[] = [];
      let spent = 0;
      let labour = 0;
      let last = screen;
      try {
        for (const st of p.steps) {
          if (st.entry && fit) {
            const slot = target(st.entry.kind, prefer);
            if (!slot) throw new Error(`no slot for ${st.mod.name} on the car`);
            const r = api.mechanic(slot.id, st.owned ? { part: st.owned.part.uid } : { buy: st.mod.id }, oldParts === "sell");
            spent += r.job.bought;
            labour += r.job.fee;
            last = r.workshop;
          } else if (st.entry) {
            if (!st.owned) {
              last = api.buy(st.entry.kind, st.mod.id);
              spent += st.entry.price;
            }
          } else {
            if (!api.install) throw new Error("upgrades cannot be installed here");
            last = api.install(st.mod.id);
            spent += st.mod.price;
          }
          done.push(st.mod.name);
        }
        setError(null);
        setNotice(
          `${fit ? "Bought and fitted" : "Bought"} ${done.join(" + ")}: $${spent.toLocaleString()}${labour ? ` + labour $${labour.toLocaleString()}` : ""}${fit ? "." : ", in the inventory to fit in that order."}`,
        );
      } catch (e) {
        const msg = String((e as Error).message ?? e).replace(/^(Error: )?wp_wasm: /, "");
        setError(done.length ? `${msg} (already bought: ${done.join(", ")})` : msg);
        setNotice(null);
        try {
          last = api.screen();
        } catch {
          // Keep the last screen a step returned.
        }
      }
      setScreen(last);
      host.changed(last);
    },
  };
  const back = useRef(host.back);
  back.current = host.back;
  const modal = useRef(false);
  modal.current = !!catalog || stand;
  // A part picked on the stand engine in 3D opens the stand list at it.
  useEffect(() => {
    if (state.standSelected) setStand(true);
    host.highlightStand(state.standSelected ? [state.standSelected] : []);
  }, [state.standSelected, screen]);
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
        {cam("rear", "Rear")}
        {cam("under", "Underside")}
        {cam("wheels", "Wheels")}
        <div class="wp-dim wp-mono wp-upper">Lift</div>
        <Button size="sm" pressed={lift} onClick={() => (host.lift(!lift), setLift(!lift))}>
          {lift ? "Lower car" : "Raise car"}
        </Button>
        <div class="wp-dim wp-mono wp-upper">Parts</div>
        <Button size="sm" onClick={() => openCatalog({ kind: null, slot: null })}>
          Catalog
        </Button>
        <Button size="sm" pressed={list} onClick={() => setList(!list)}>
          Part list
        </Button>
        {api.paintShop && (
          <Button size="sm" pressed={paint} testid="ws-paint-shop" onClick={() => (setPaint(!paint), !paint && host.camera("overview"))} title="respray, vinyls, finishes and cosmetic parts">
            Paint shop
          </Button>
        )}
        {cam("stand", "Engine stand")}
        <Button size="sm" pressed={stand} onClick={() => (setStand(!stand), !stand && host.camera("stand"))} title="strip and build a loose engine">
          Stand parts{screen.stands.length > 0 ? ` (${screen.stands.length})` : ""}
        </Button>
      </nav>
      {list && <PartsList screen={screen} state={state} onClose={() => setList(false)} />}
      {state.standHover && (
        <div class="wp-ws-hover wp-mono">
          {(() => {
            const s = screen.stands[0]?.slots.find((x) => x.id === state.standHover);
            return s ? `${s.name}${s.fitted ? ` · ${pct(s.fitted.condition)}` : ""}` : state.standHover;
          })()}
        </div>
      )}
      {hover && hover.id !== state.selected && (
        <div class="wp-ws-hover wp-mono">
          {hover.name}
          {hover.fitted ? ` · ${pct(hover.fitted.condition)}` : " · empty"}
          {(() => {
            // What upgrades cost here, so a part worth clicking shows before the click.
            const prices = [
              ...screen.catalog.filter((c) => c.kind === hover.kind && c.grade !== null && !c.reason && c.grade !== hover.fitted?.grade).map((c) => c.price),
              ...(tree ? boltOns(tree, screen.catalog).filter((m) => !m.installed && upgradeGroup(m) === hover.group).map((m) => m.price) : []),
            ];
            return prices.length ? ` · upgrades from $${Math.min(...prices).toLocaleString()}` : "";
          })()}
        </div>
      )}
      <aside class="wp-ws-right">
        {error && (
          <p class="wp-danger wp-ws-notice" data-testid="ws-error">
            {error}
          </p>
        )}
        {notice && (
          <p class="wp-ok wp-ws-notice" data-testid="ws-notice">
            {notice}
          </p>
        )}
        {paint && api.paintShop ? (
          <PaintShop api={api.paintShop} onScreen={(next) => (setScreen(next), host.changed(next))} onClose={() => setPaint(false)} />
        ) : slot ? (
          <PartCard
            slot={slot}
            screen={screen}
            run={run}
            hire={hire}
            api={api}
            slotName={slotName}
            openCatalog={(kind) => openCatalog({ kind, slot: slot.id })}
            oldParts={oldParts}
            setOldParts={setOldParts}
            shop={shop}
          />
        ) : (
          <Panel title="Workshop">
            <p class="wp-dim">Click a part on the car to work on it or buy upgrades for it. Drag to look around, scroll to zoom.</p>
            <p class="wp-dim">Do it yourself for free, or pay the mechanic to swap a part: they take off whatever is in the way and put it all back.</p>
            <div class="wp-row wp-ws-oldparts" style={{ gap: "0.4em" }}>
              <span class="wp-dim">Old parts from mechanic jobs:</span>
              {OLD_PARTS.map((o) => (
                <Button key={o} size="sm" pressed={oldParts === o} onClick={() => setOldParts(o)}>
                  {o === "keep" ? "Keep" : "Sell"}
                </Button>
              ))}
            </div>
            {screen.upgrades.length > 0 && <p class="wp-dim">Other upgrades: {screen.upgrades.join(", ")}</p>}
          </Panel>
        )}
      </aside>
      <InventoryStrip screen={screen} run={run} api={api} host={host} state={state} />
      {catalog && (
        <Catalog shop={shop} kind={catalog.kind} slot={catalog.slot} notice={notice} error={error} onClose={() => setCatalog(null)} />
      )}
      {stand && !catalog && (
        <EngineStand screen={screen} run={run} api={api} state={state} host={host} onClose={() => (setStand(false), state.set({ standSelected: null }))} />
      )}
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
