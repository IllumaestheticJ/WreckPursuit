/**
 * Career garage (machine G): the between-runs screen of the browser career. Driven entirely by
 * a `CareerApi` (wp_wasm's `career_*` exports, wired by the testbed), so it holds no game
 * state of its own: every action returns the new `CareerView` and the screen redraws.
 *
 * Tabs: Cars (condition, value, repair per item at a district shop, insurance, sell, mods,
 * the free four-slider tune),
 * Dealer (roster cars by class band, the economy-floor beater), Impound (release or let the
 * auction countdown run), Ledger. The head has cash / debt / rep / bounty / carried heat and
 * starts the next run (free drive or city chase) with the active car.
 */
import { render, type ComponentChildren } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { Badge, Button, Delta, Meter, Money, Panel, severityColor, Stat, Tabs } from "../ui/kit";
import { pct, words } from "../ui/format";
import type {
  CareerApi,
  CarTune,
  CareerView,
  CarView,
  Condition,
  DamageMode,
  ModOption,
  ModTreeView,
  RepairItem,
  ShopKind,
  ShopRequest,
  StatsView,
} from "./types";
import { pickTextFile } from "./store";
import { isStockTune, normalizeTune, STOCK_TUNE, TUNE_SLIDERS, TUNE_STEPS, tuneSummary } from "./tune";
import "./garage.css";

type Tab = "cars" | "dealer" | "pound" | "ledger";

const itemKey = (i: RepairItem) => JSON.stringify(i);
const fmt = {
  pi: (v: number) => v.toFixed(0),
  kg: (v: number) => `${v.toFixed(0)} kg`,
  s: (v: number) => `${v.toFixed(2)} s`,
  kmh: (v: number) => `${v.toFixed(0)} km/h`,
  g: (v: number) => `${v.toFixed(2)} g`,
};

/** Ledger reason → short label (falls back to the reason with spaces). */
const REASON: Record<string, string> = {
  AccountOpened: "Account opened",
  CarPurchased: "Car bought",
  CarSold: "Car sold",
  FloorCarIssued: "Street beater",
  ModInstalled: "Mod installed",
  ModRemoved: "Mod removed (refund)",
  RepairPaid: "Repair",
  ImpoundRetrieved: "Pound fee",
  CarImpounded: "Impounded",
  CarAuctioned: "Auctioned",
  Fine: "Fine",
  Bail: "Bail",
  Towing: "Towing",
  Escape: "Escape",
  InsurancePurchased: "Insurance",
  InsurancePremium: "Premium",
  InsuranceLapsed: "Insurance ended",
  InsuranceClaim: "Insurance claim",
  DamageRecorded: "Damage recorded",
  CarScrapped: "Scrapped",
  ShopReported: "Shop reported you",
};

function StatGrid({ now, after }: { now: StatsView; after?: StatsView | null }) {
  const a = after ?? now;
  return (
    <div class="wp-cg-stats wp-mono">
      <span class="wp-dim">PI</span>
      <span>
        <Delta before={now.class_index} after={a.class_index} format={fmt.pi} /> <span class="wp-dim">({a.bracket})</span>
      </span>
      <span class="wp-dim">Mass</span>
      <Delta before={now.mass_kg} after={a.mass_kg} format={fmt.kg} higherIsBetter={false} epsilon={0.5} />
      <span class="wp-dim">0–100</span>
      <Delta before={now.est_0_100_s} after={a.est_0_100_s} format={fmt.s} higherIsBetter={false} epsilon={0.005} />
      <span class="wp-dim">Top</span>
      <Delta before={now.top_speed_kmh} after={a.top_speed_kmh} format={fmt.kmh} epsilon={0.5} />
      <span class="wp-dim">Grip</span>
      <Delta before={now.lateral_g} after={a.lateral_g} format={fmt.g} epsilon={0.005} />
      <span class="wp-dim">Brakes</span>
      <Delta before={now.brake_g} after={a.brake_g} format={fmt.g} epsilon={0.005} />
    </div>
  );
}

function CarCard({ car, selected, onSelect }: { car: CarView; selected: boolean; onSelect: () => void }) {
  return (
    <button type="button" class="wp-cg-card" aria-selected={selected} onClick={onSelect} data-testid={`car-${car.uid}`}>
      <div class="wp-spread">
        <b class="wp-upper">{car.name}</b>
        <span class="wp-dim wp-mono">{words(car.class)}</span>
      </div>
      <Meter value={car.health} color={severityColor(1 - car.health)} label="Condition" />
      <div class="wp-row" style={{ gap: "0.3em", flexWrap: "wrap" }}>
        {car.active && <Badge tone="ok">Active</Badge>}
        {car.location === "Impound" && <Badge tone="danger">Pound</Badge>}
        {car.total_loss && (
          <Badge tone="danger" solid>
            Wrecked
          </Badge>
        )}
        {car.repair_lines.length > 0 && !car.total_loss && <Badge tone="warn">{car.repair_lines.length} repairs</Badge>}
        {car.insurance && <Badge tone="info">{car.insurance}</Badge>}
        {car.mods.length > 0 && <Badge>{car.mods.length} mods</Badge>}
        {car.tune && !isStockTune(car.tune) && <Badge title={tuneSummary(car.tune)}>tuned</Badge>}
        <span class="wp-dim wp-mono" style={{ marginLeft: "auto" }}>
          <Money value={car.value} />
        </span>
      </div>
    </button>
  );
}

function RepairPanel(props: { car: CarView; view: CareerView; api: CareerApi; run: (f: () => CareerView) => void }) {
  const { car, view, api } = props;
  const [district, setDistrict] = useState("Residential");
  const [shop, setShop] = useState<ShopKind>("Legit");
  const [picked, setPicked] = useState<string[]>([]);
  const [scrapAsk, setScrapAsk] = useState(false);
  if (car.location !== "Garage") return null;
  const items = car.repair_lines.filter((l) => picked.includes(itemKey(l.item))).map((l) => l.item);
  const req = (option: ShopRequest["option"]): ShopRequest => ({ option, district, shop });
  const quote = (option: ShopRequest["option"]) => {
    try {
      return api.quote(car.uid, req(option));
    } catch {
      return null;
    }
  };
  const all = car.repair_lines.length ? quote("RepairAll") : null;
  const some = items.length ? quote({ RepairSelected: items }) : null;
  const wanted = view.carried_level !== "None";
  return (
    <Panel title="Repair" tag={`${view.mode} prices`}>
      {car.repair_lines.length === 0 ? (
        <p class="wp-dim">Nothing to repair.</p>
      ) : (
        <>
          <div class="wp-row" style={{ gap: "0.6em", flexWrap: "wrap", marginBottom: "0.5em" }}>
            <label class="wp-mono wp-dim">
              district{" "}
              <select class="wp-select" value={district} onChange={(e) => setDistrict((e.target as HTMLSelectElement).value)} data-testid="repair-district">
                {view.districts.map((d) => (
                  <option value={d.id} key={d.id}>
                    {words(d.id)} ×{d.labour.toFixed(2)}
                  </option>
                ))}
              </select>
            </label>
            <Button size="sm" pressed={shop === "Legit"} onClick={() => setShop("Legit")}>
              Legit shop
            </Button>
            <Button size="sm" pressed={shop === "Crooked"} onClick={() => setShop("Crooked")} title="never reports you; marks up labour, more when you are wanted">
              Crooked shop
            </Button>
          </div>
          {wanted && shop === "Legit" && <p class="wp-warn">You are wanted ({view.carried_level}): a legit shop will report you.</p>}
          <table class="wp-table" data-testid="repair-lines">
            <thead>
              <tr>
                <th />
                <th>Item</th>
                <th class="num">Damage</th>
                <th class="num">Price</th>
              </tr>
            </thead>
            <tbody>
              {car.repair_lines.map((l) => {
                const k = itemKey(l.item);
                const on = picked.includes(k);
                return (
                  <tr key={k}>
                    <td>
                      <input type="checkbox" checked={on} onChange={() => setPicked(on ? picked.filter((p) => p !== k) : [...picked, k])} />
                    </td>
                    <td>{l.label}</td>
                    <td class="num" style={{ color: severityColor(l.severity) }}>
                      {pct(l.severity)}
                    </td>
                    <td class="num">
                      <Money value={l.price} />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          {car.damage_before_capture && <p class="wp-warn">This car was caught by the police with this damage: insurance will not pay for it.</p>}
          <div class="wp-row" style={{ gap: "0.5em", marginTop: "0.6em", flexWrap: "wrap" }}>
            <Button variant="primary" disabled={!all || all.player_pays > view.cash} onClick={() => props.run(() => api.repair(car.uid, req("RepairAll")))}>
              Repair all {all && <Money value={all.player_pays} />}
            </Button>
            <Button disabled={!some || some.player_pays > view.cash} onClick={() => (props.run(() => api.repair(car.uid, req({ RepairSelected: items }))), setPicked([]))}>
              Repair selected {some && <Money value={some.player_pays} />}
            </Button>
            <Button variant="danger" pressed={scrapAsk} onClick={() => setScrapAsk(!scrapAsk)} title="sell the car for salvage">
              Scrap…
            </Button>
          </div>
          {scrapAsk && (
            <Confirm
              text={
                <>
                  Scrap it for <Money value={car.scrap_value} /> (bought for <Money value={car.paid} />)? The car is gone for good.
                </>
              }
              yes="Scrap"
              onYes={() => (setScrapAsk(false), props.run(() => api.repair(car.uid, req("Scrap"))))}
              onNo={() => setScrapAsk(false)}
            />
          )}
          {all && all.insurance_covers > 0 && (
            <p class="wp-dim">
              Insurance pays <Money value={all.insurance_covers} /> of <Money value={all.gross} />.
            </p>
          )}
        </>
      )}
    </Panel>
  );
}

/** Inline "are you sure" row for a destructive action (sell, scrap). */
function Confirm(props: { text: ComponentChildren; yes: string; onYes: () => void; onNo: () => void }) {
  return (
    <div class="wp-row wp-cg-confirm" data-testid="career-confirm" style={{ gap: "0.5em", marginTop: "0.5em", flexWrap: "wrap", alignItems: "center" }}>
      <span>{props.text}</span>
      <Button size="sm" variant="danger" onClick={props.onYes}>
        {props.yes}
      </Button>
      <Button size="sm" onClick={props.onNo}>
        Cancel
      </Button>
    </div>
  );
}

/** One line per tier, from the data: "Basic: 50 % of the repair bill above $500, $X/night." */
function insuranceNote(view: CareerView, car: CarView): string {
  const tiers = view.insurance.map((t) => {
    const premium = Math.max(1, Math.round(t.premium_per_day_frac * car.value));
    return `${words(t.id)}: pays ${pct(t.coverage)} of the repair bill above $${t.deductible}, $${premium}/night`;
  });
  return `${tiers.join(". ")}. Covers damage taken while wanted too; only damage still on the car when you are caught is not paid.`;
}

function InsurancePanel(props: { car: CarView; view: CareerView; api: CareerApi; run: (f: () => CareerView) => void }) {
  const { car, view, api } = props;
  if (car.location !== "Garage") return null;
  return (
    <Panel title="Insurance" tag={car.insurance ?? "none"}>
      <div class="wp-row" style={{ gap: "0.5em", flexWrap: "wrap" }}>
        <Button size="sm" pressed={!car.insurance} onClick={() => car.insurance && props.run(() => api.insure(car.uid, ""))}>
          None
        </Button>
        {view.insurance.map((t) => (
          <Button
            size="sm"
            key={t.id}
            pressed={car.insurance === t.id}
            onClick={() => car.insurance !== t.id && props.run(() => api.insure(car.uid, t.id))}
            title={`covers ${pct(t.coverage)} after a ${t.deductible} deductible, chases included; not damage still on the car when you are caught`}
          >
            {words(t.id)} · <Money value={Math.max(1, Math.round(t.premium_per_day_frac * car.value))} />
            /night
          </Button>
        ))}
      </div>
      <p class="wp-dim" data-testid="insurance-note" style={{ fontSize: "0.85em", margin: "0.4em 0 0" }}>
        {insuranceNote(view, car)}
      </p>
    </Panel>
  );
}

const GROUPS: ModOption["group"][] = ["performance", "durability", "tool", "cosmetic"];

function ModTree(props: { car: CarView; api: CareerApi; run: (f: () => CareerView) => void; onBack: () => void }) {
  const { car, api } = props;
  const [group, setGroup] = useState<ModOption["group"]>("performance");
  const [hover, setHover] = useState<string | null>(null);
  let tree: ModTreeView | null = null;
  let error: string | null = null;
  try {
    tree = api.mods(car.uid);
  } catch (e) {
    error = String((e as Error).message ?? e);
  }
  if (!tree) return <Panel title="Mods">{error}</Panel>;
  const shown = tree.mods.filter((m) => m.group === group);
  const focus = tree.mods.find((m) => m.id === hover) ?? null;
  return (
    <div class="wp-cg-mods" data-testid="mod-tree">
      <Panel title={`Mods · ${car.name}`} tag={<Button size="sm" onClick={props.onBack}>Back</Button>}>
        <Tabs tabs={GROUPS.map((g) => ({ id: g, label: words(g) }))} value={group} onChange={setGroup} />
        <table class="wp-table">
          <thead>
            <tr>
              <th>Mod</th>
              <th>Slot</th>
              <th class="num">Price</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {shown.map((m) => (
              <tr key={m.id} onMouseEnter={() => setHover(m.id)} data-mod={m.id}>
                <td>
                  <b>{m.name}</b> <span class="wp-dim wp-mono">T{m.tier}</span>
                  <div class="wp-dim" style={{ fontSize: "0.8em" }}>
                    {m.tradeoff}
                  </div>
                  {!m.allowed && m.reason && (
                    <div class="wp-warn" style={{ fontSize: "0.8em" }}>
                      {m.reason}
                    </div>
                  )}
                </td>
                <td class="wp-dim">{words(m.slot)}</td>
                <td class="num">
                  <Money value={m.price} />
                </td>
                <td>
                  {m.installed ? (
                    <Button size="sm" disabled={!m.allowed} onClick={() => props.run(() => api.uninstall(car.uid, m.id))}>
                      Remove
                    </Button>
                  ) : (
                    <Button size="sm" variant="primary" disabled={!m.allowed} onClick={() => props.run(() => api.install(car.uid, m.id))}>
                      {m.replaces ? "Swap" : "Install"}
                    </Button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Panel>
      <Panel title={focus ? (focus.installed ? `Without ${focus.name}` : `With ${focus.name}`) : "Build"} tag={focus ? `T${focus.tier}` : undefined}>
        <StatGrid now={tree.stats} after={focus?.stats_after} />
        {focus?.replaces && <p class="wp-dim">Replaces {focus.replaces} (refunded at 40 %).</p>}
        {focus && focus.requires.length > 0 && <p class="wp-dim">Needs: {focus.requires.join(", ")}</p>}
        <p class="wp-dim" style={{ fontSize: "0.8em" }}>
          Hover a mod to see what it changes. Durability adds mass; removing a mod refunds 40 %.
        </p>
      </Panel>
    </div>
  );
}

/**
 * The free tune page (reference-game style): four 5-step sliders, 3 = stock, a line under each
 * saying what it does, and Reset. Every change is saved at once (no money) and applies when the
 * next run spawns the car. Range inputs: Tab moves between sliders, arrow keys step them.
 */
function TunePage(props: { car: CarView; api: CareerApi; run: (f: () => CareerView) => void; onBack: () => void }) {
  const { car, api } = props;
  const tune = normalizeTune(car.tune);
  const set = (next: CarTune) => props.run(() => api.setTune(car.uid, next));
  return (
    <div class="wp-cg-tune" data-testid="tune-page">
      <Panel title={`Tune · ${car.name}`} tag={<Button size="sm" onClick={props.onBack}>Back</Button>}>
        <p class="wp-dim" style={{ marginTop: 0 }}>
          Free. Saved with the car and used from the next run. 3 is the car as it left the factory.
        </p>
        {TUNE_SLIDERS.map((s) => {
          const v = tune[s.key];
          return (
            <div class="wp-cg-tune-row" key={s.key} data-tune={s.key}>
              <div class="wp-spread">
                <b>{s.label}</b>
                <span class="wp-mono" data-testid={`tune-${s.key}-value`}>
                  {v} · {s.steps[v - 1]}
                </span>
              </div>
              <div class="wp-row wp-cg-tune-ctl">
                <span class="wp-dim wp-mono">{s.low}</span>
                <input
                  type="range"
                  class="wp-slider"
                  min={1}
                  max={TUNE_STEPS}
                  step={1}
                  value={v}
                  aria-label={s.label}
                  aria-valuetext={`${v} of ${TUNE_STEPS}, ${s.steps[v - 1]}`}
                  data-testid={`tune-${s.key}`}
                  onChange={(e) => {
                    const n = Number((e.target as HTMLInputElement).value);
                    if (n !== v) set({ ...tune, [s.key]: n });
                  }}
                />
                <span class="wp-dim wp-mono">{s.high}</span>
              </div>
              <div class="wp-cg-tune-ticks wp-mono wp-dim" aria-hidden="true">
                {s.steps.map((label, i) => (
                  <span key={label} class={i + 1 === v ? "is-on" : undefined}>
                    {i + 1}
                  </span>
                ))}
              </div>
              <small class="wp-dim">{s.hint}</small>
            </div>
          );
        })}
        <div class="wp-row" style={{ gap: "0.5em", marginTop: "0.6em", alignItems: "center" }}>
          <Button disabled={isStockTune(tune)} onClick={() => set({ ...STOCK_TUNE })}>
            Reset to stock
          </Button>
          <span class="wp-dim wp-mono" data-testid="tune-summary">
            {tuneSummary(tune)}
          </span>
        </div>
      </Panel>
    </div>
  );
}

function CarsTab(props: { view: CareerView; api: CareerApi; run: (f: () => CareerView) => void }) {
  const { view, api } = props;
  const [sel, setSel] = useState<number | null>(view.active_car);
  const [mods, setMods] = useState(false);
  const [tuning, setTuning] = useState(false);
  const [sellAsk, setSellAsk] = useState(false);
  const car = view.cars.find((c) => c.uid === sel) ?? view.cars.find((c) => c.active) ?? view.cars[0];
  if (!car) {
    return (
      <Panel title="No cars">
        <p>Buy one at the dealer{view.floor_car ? ", or take the street beater" : ""}.</p>
        {view.floor_car && (
          <Button variant="primary" onClick={() => props.run(() => api.floorCar())}>
            Street beater (<Money value={view.floor_car[1]} />)
          </Button>
        )}
      </Panel>
    );
  }
  if (mods && car.location === "Garage") return <ModTree car={car} api={api} run={props.run} onBack={() => setMods(false)} />;
  if (tuning && car.location === "Garage") return <TunePage car={car} api={api} run={props.run} onBack={() => setTuning(false)} />;
  return (
    <div class="wp-cg-cars">
      <div class="wp-cg-list" role="listbox" aria-label="Your cars">
        {view.cars.map((c) => (
          <CarCard key={c.uid} car={c} selected={c.uid === car.uid} onSelect={() => (setSel(c.uid), setSellAsk(false))} />
        ))}
      </div>
      <div class="wp-cg-detail">
        <Panel title={car.name} tag={words(car.class)}>
          <div class="wp-cg-hero">
            <Stat label="Condition" value={pct(car.health)} tone={car.total_loss ? "danger" : car.health > 0.8 ? "ok" : "warn"} />
            <Stat label="Value" value={<Money value={car.value} />} />
            <Stat label="Dealer pays" value={<Money value={car.sale_price} />} />
          </div>
          {car.stats && <StatGrid now={car.stats} />}
          <div class="wp-mono wp-dim" style={{ fontSize: "0.85em", margin: "0.4em 0" }}>
            {car.mods.length ? car.mods.map((m) => m.name).join(" · ") : "Stock"}
            {car.tune && !isStockTune(car.tune) && <> · tune: {tuneSummary(car.tune)}</>}
          </div>
          <div class="wp-row" style={{ gap: "0.5em", flexWrap: "wrap" }}>
            <Button variant="primary" disabled={car.active || car.location !== "Garage"} onClick={() => props.run(() => api.setActive(car.uid))}>
              {car.active ? "Active car" : "Drive this car"}
            </Button>
            {api.workshop && (
              <Button disabled={car.location !== "Garage"} onClick={() => api.workshop!(car.uid)} title="work on the car yourself: take parts off, fit new ones">
                Workshop
              </Button>
            )}
            <Button disabled={car.location !== "Garage"} onClick={() => setMods(true)}>
              Mods
            </Button>
            <Button disabled={car.location !== "Garage"} onClick={() => setTuning(true)} title="free: suspension, gearing, differential, brake balance">
              Tune
            </Button>
            <Button variant="danger" disabled={car.location !== "Garage"} pressed={sellAsk} onClick={() => setSellAsk(!sellAsk)}>
              Sell (+<Money value={car.sale_price} />)
            </Button>
          </div>
          {sellAsk && car.location === "Garage" && (
            <Confirm
              text={
                <>
                  Sell it to the dealer for <Money value={car.sale_price} />? You paid <Money value={car.paid} /> for the car
                  {car.mods.length ? " (mods return 40 % of their price)" : ""}.
                </>
              }
              yes="Sell"
              onYes={() => (setSellAsk(false), props.run(() => api.sell(car.uid)))}
              onNo={() => setSellAsk(false)}
            />
          )}
          {car.impound && (
            <p class="wp-warn">
              In the pound: <Money value={car.impound.fee} /> today, +<Money value={car.impound.fee_per_day} /> a night, auctioned in{" "}
              {car.impound.days_until_auction + 1} nights.
            </p>
          )}
        </Panel>
        <RepairPanel key={`r${car.uid}`} car={car} view={view} api={api} run={props.run} />
        <InsurancePanel car={car} view={view} api={api} run={props.run} />
      </div>
    </div>
  );
}

function DealerTab(props: { view: CareerView; api: CareerApi; run: (f: () => CareerView) => void }) {
  const { view, api } = props;
  const buy = (id: string, c: Condition) => props.run(() => api.buy(id, c));
  return (
    <Panel title="Dealer" tag="street roster">
      {view.floor_car && (
        <p>
          No usable car? The yard always has a beater:{" "}
          <Button size="sm" variant="primary" onClick={() => props.run(() => api.floorCar())}>
            Street beater (<Money value={view.floor_car[1]} />)
          </Button>
        </p>
      )}
      <table class="wp-table" data-testid="dealer">
        <thead>
          <tr>
            <th>Car</th>
            <th>Class</th>
            <th class="num">PI</th>
            <th class="num">0–100</th>
            <th class="num">New</th>
            <th class="num">Used</th>
            <th class="num">Worn</th>
          </tr>
        </thead>
        <tbody>
          {view.dealer.map((d) => (
            <tr key={d.def_id} data-car={d.def_id}>
              <td>
                <b>{d.name}</b>
              </td>
              <td class="wp-dim">{words(d.class)}</td>
              <td class="num">
                {d.stats.class_index} <span class="wp-dim">{d.stats.bracket}</span>
              </td>
              <td class="num">{d.stats.est_0_100_s.toFixed(1)} s</td>
              {(["New", "Used", "Worn"] as Condition[]).map((c) => {
                const price = c === "New" ? d.price_new : c === "Used" ? d.price_used : d.price_worn;
                return (
                  <td class="num" key={c}>
                    <Button size="sm" disabled={price > view.cash} onClick={() => buy(d.def_id, c)} title={`buy ${c.toLowerCase()}`}>
                      <Money value={price} />
                    </Button>
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
      <p class="wp-dim" style={{ fontSize: "0.8em" }}>
        Used cars start at 90 % condition, worn ones at 75 % (cosmetic wear: nothing to repair).
      </p>
    </Panel>
  );
}

function PoundTab(props: { view: CareerView; api: CareerApi; run: (f: () => CareerView) => void }) {
  const { view, api } = props;
  const inPound = view.cars.filter((c) => c.impound);
  return (
    <Panel title="Impound lot" tag={`${inPound.length} cars`}>
      {inPound.length === 0 && <p class="wp-dim">Nothing in the impound.</p>}
      {inPound.map((c) => {
        const i = c.impound!;
        return (
          <div key={c.uid} class="wp-cg-pound" data-testid={`pound-${c.uid}`}>
            <div class="wp-spread">
              <b class="wp-upper">{c.name}</b>
              <Badge tone="danger">since night {i.since_day}</Badge>
            </div>
            <div class="wp-cg-hero">
              <Stat label="Fee today" value={<Money value={i.fee} />} tone="warn" />
              <Stat label="Per night" value={<Money value={i.fee_per_day} />} />
              <Stat label="Auction in" value={`${i.days_until_auction + 1}`} unit="nights" tone={i.days_until_auction < 2 ? "danger" : undefined} />
              <Stat label="Condition" value={pct(c.health)} />
            </div>
            <Button variant="primary" disabled={!i.affordable} onClick={() => props.run(() => api.release(c.uid))}>
              Pay & release (<Money value={i.fee} />)
            </Button>
          </div>
        );
      })}
      <p class="wp-dim">Letting nights pass grows the fee; after the countdown the car is auctioned and you get a small share of what it fetches.</p>
      <Button onClick={() => props.run(() => api.nextNight())}>
        Let a night pass
      </Button>
    </Panel>
  );
}

function LedgerTab({ view }: { view: CareerView }) {
  const lines = [...view.ledger].reverse().filter((l) => l.cash_delta !== 0 || l.bounty_delta !== 0 || l.rep_delta !== 0);
  return (
    <Panel title="Ledger" tag={`night ${view.day}`}>
      <div class="wp-cg-hero">
        <Stat label="Runs" value={view.stats.runs} />
        <Stat label="Escapes" value={view.escapes} tone="ok" />
        <Stat label="Busted" value={view.captures} tone="danger" />
        <Stat label="Wrecked" value={view.stats.runs_wrecked} />
        <Stat label="Net worth" value={<Money value={view.net_worth} />} tone="cash" />
      </div>
      <table class="wp-table" data-testid="ledger">
        <tbody>
          {lines.map((l) => (
            <tr key={l.seq}>
              <td class="wp-dim wp-mono">N{l.day}</td>
              <td>{REASON[l.reason] ?? words(l.reason)}</td>
              <td class="num">{l.cash_delta !== 0 ? <Money value={l.cash_delta} signed tone /> : l.bounty_delta !== 0 ? `bounty ${l.bounty_delta > 0 ? "+" : ""}${l.bounty_delta}` : `rep ${l.rep_delta > 0 ? "+" : ""}${l.rep_delta}`}</td>
              <td class="num wp-dim">
                <Money value={l.cash_after} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </Panel>
  );
}

function NewCareer(props: { api: CareerApi; run: (f: () => CareerView) => void; onDone: () => void }) {
  const [mode, setMode] = useState<DamageMode>("Arcade");
  return (
    <Panel title="New career" tag="replaces the current one">
      <p>
        Arcade: one hard hit degrades, several end the car; repairs at half price. Simulation: one hard hit at speed is a total loss, and a
        total-loss bust means the car is gone.
      </p>
      <div class="wp-row" style={{ gap: "0.5em" }}>
        <Button size="sm" pressed={mode === "Arcade"} onClick={() => setMode("Arcade")}>
          Arcade
        </Button>
        <Button size="sm" pressed={mode === "Simulation"} onClick={() => setMode("Simulation")}>
          Simulation
        </Button>
        <Button variant="danger" onClick={() => (props.run(() => props.api.newCareer(mode)), props.onDone())}>
          Start over in {mode}
        </Button>
        <Button onClick={props.onDone}>Cancel</Button>
      </div>
    </Panel>
  );
}

function GarageScreen(props: { api: CareerApi; initial: CareerView; message: string | null }) {
  const { api } = props;
  const [view, setView] = useState<CareerView>(props.initial);
  const [tab, setTab] = useState<Tab>(props.initial.cars.length ? "cars" : "dealer");
  const [error, setError] = useState<string | null>(props.message);
  const [fresh, setFresh] = useState(false);
  const run = (f: () => CareerView) => {
    try {
      setView(f());
      setError(null);
    } catch (e) {
      setError(String((e as Error).message ?? e).replace(/^wp_wasm: /, ""));
    }
  };
  const importSave = () => {
    pickTextFile().then(
      (text) => run(() => api.importSave(text)),
      () => {},
    );
  };
  // Escape closes the garage.
  const close = useRef(api.close);
  close.current = api.close;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        close.current();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
  const pound = view.cars.filter((c) => c.impound).length;
  return (
    <div class="wp-screen wp-cg" data-testid="career-garage">
      <header class="wp-head">
        <h1>
          <small>{`// GARAGE · NIGHT ${view.day} · ${view.mode.toUpperCase()}`}</small>
          Garage
        </h1>
        <div class="wp-head-right">
          <span>
            CASH <Money value={view.cash} tone />
          </span>
          {view.debt > 0 && <Badge tone="danger">debt {view.debt.toLocaleString("en-US")}</Badge>}
          <span>
            REP <span class="wp-ok">{view.rep.toLocaleString("en-US")}</span>
          </span>
          <span>BOUNTY {view.bounty.toLocaleString("en-US")}</span>
          {view.carried_level !== "None" && <Badge tone="warn">wanted {view.carried_level}</Badge>}
          <Button size="sm" onClick={api.close} hotkey="Esc">
            Close
          </Button>
        </div>
      </header>
      <div class="wp-row wp-cg-start" style={{ gap: "0.5em", flexWrap: "wrap", marginBottom: "0.8em" }}>
        <Button variant="primary" disabled={!view.can_start} onClick={() => api.startRun("chase")}>
          City chase
        </Button>
        <Button disabled={!view.can_start} onClick={() => api.startRun("free")}>
          Free drive
        </Button>
        {view.start_blocker && <span class="wp-warn">{view.start_blocker}</span>}
        <span style={{ marginLeft: "auto" }} />
        <Button size="sm" onClick={() => api.exportSave()}>
          Export save
        </Button>
        <Button size="sm" onClick={importSave}>
          Import save
        </Button>
        <Button size="sm" variant="ghost" onClick={() => setFresh(true)}>
          New career
        </Button>
      </div>
      {error && (
        <p class="wp-danger" data-testid="garage-error">
          {error}
        </p>
      )}
      {fresh && <NewCareer api={api} run={run} onDone={() => setFresh(false)} />}
      <Tabs
        tabs={[
          { id: "cars", label: `Cars (${view.cars.length})` },
          { id: "dealer", label: "Dealer" },
          { id: "pound", label: pound ? `Impound (${pound})` : "Impound" },
          { id: "ledger", label: "Ledger" },
        ]}
        value={tab}
        onChange={setTab}
      />
      <div style={{ marginTop: "0.8em" }}>
        {tab === "cars" && <CarsTab view={view} api={api} run={run} />}
        {tab === "dealer" && <DealerTab view={view} api={api} run={run} />}
        {tab === "pound" && <PoundTab view={view} api={api} run={run} />}
        {tab === "ledger" && <LedgerTab view={view} />}
      </div>
    </div>
  );
}

/** Mounts the career garage into `el` (a full-screen overlay while open). */
export class CareerGarageView {
  /** The garage is showing. */
  open = false;
  private key = 0;

  constructor(
    private readonly el: HTMLElement,
    private readonly api: CareerApi,
  ) {}

  /** Show the garage (re-reads the career). `message` is shown at the top (e.g. why a run
   * could not start). */
  show(message: string | null = null): void {
    this.open = true;
    this.key++;
    const view = this.api.view();
    render(
      <div class="wp-ui wp-root" style={{ position: "absolute", inset: 0, pointerEvents: "auto", zIndex: 80 }}>
        <GarageScreen key={this.key} api={this.api} initial={view} message={message} />
      </div>,
      this.el,
    );
  }

  /** Hide it. */
  hide(): void {
    this.open = false;
    render(null, this.el);
  }
}
