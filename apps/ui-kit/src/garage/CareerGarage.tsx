/**
 * Career garage (machine G): the between-runs screen of the browser career. Driven entirely by
 * a `CareerApi` (wp_wasm's `career_*` exports, wired by the testbed), so it holds no game
 * state of its own: every action returns the new `CareerView` and the screen redraws.
 *
 * Tabs: Cars (condition, value, repair per item at a district shop, tyres, insurance, sell,
 * the workshop, the free four-slider tune; mods are bought in the workshop only), Car lots (dealerships, used lots, junkyards: list today's
 * stock or visit the lot; classes above the player's rep tier are locked; the economy-floor
 * beater), Impound (release or let the auction countdown run), Career (rep tier, progress to
 * the next, what each tier unlocks), Ledger. The head has cash / debt / rep /
 * bounty / carried heat and
 * starts the next run (free drive or night run) with the active car.
 */
import { render, type ComponentChildren } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";
import { Badge, Button, Confirm as ConfirmRow, Delta, Meter, Money, Panel, severityColor, Stat, Tabs } from "../ui/kit";
import { pct, words } from "../ui/format";
import type {
  CareerApi,
  CarTune,
  CareerView,
  CareerFeature,
  CarView,
  DamageMode,
  LotKind,
  LotStockView,
  RepairItem,
  ShopKind,
  ShopRequest,
  ShopSite,
  StatsView,
} from "./types";
import { pickTextFile } from "./store";
import { isStockTune, normalizeTune, STOCK_TUNE, TUNE_SLIDERS, TUNE_STEPS, tuneSummary } from "./tune";
import "./garage.css";

type Tab = "cars" | "lots" | "pound" | "police" | "career" | "ledger";

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
  BountyCleared: "Bounty cleared",
  InformantTip: "Informant",
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
        <span class="wp-dim wp-mono">
          {car.rare && <Badge tone="info">rare</Badge>} {words(car.class)}
        </span>
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

/**
 * The shop the Repair panel starts on: the one the car pulled into (`at`, by name), else the
 * first shop of the main menu's spawn city, else the first shop. `null` without shops.
 */
export function startShop(shops: ShopSite[], at: string | null, homeCity: string | null): ShopSite | null {
  return shops.find((s) => s.name === at) ?? shops.find((s) => s.city === homeCity) ?? shops[0] ?? null;
}

function RepairPanel(props: { car: CarView; view: CareerView; api: CareerApi; at: string | null; run: (f: () => CareerView) => void }) {
  const { car, view, api } = props;
  // The world's shops when the host lists them (one world since 2026-10-25: each shop has its
  // district's labour rate and is legit or crooked); else the bare district and shop kind.
  const shops = view.shops ?? [];
  const [shopName, setShopName] = useState(() => startShop(shops, props.at, api.homeCity?.() ?? null)?.name ?? "");
  const site = shops.find((s) => s.name === shopName) ?? null;
  const [bareDistrict, setDistrict] = useState("Residential");
  const [bareShop, setShop] = useState<ShopKind>("Legit");
  const district = site?.district ?? bareDistrict;
  const shop = site?.kind ?? bareShop;
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
          {site ? (
            <div class="wp-row" style={{ gap: "0.6em", flexWrap: "wrap", marginBottom: "0.5em" }}>
              <label class="wp-mono wp-dim">
                shop{" "}
                <select class="wp-select" value={site.name} onChange={(e) => setShopName((e.target as HTMLSelectElement).value)} data-testid="repair-shop">
                  {shops.map((s) => (
                    <option value={s.name} key={s.name}>
                      {s.name} · {s.city_label}
                    </option>
                  ))}
                </select>
              </label>
              <span class="wp-mono wp-dim" data-testid="repair-shop-rate" title={site.kind === "Crooked" ? "never reports you; marks up labour, more when you are wanted" : "normal rates; reports wanted cars"}>
                {words(site.district)} ×{site.labour.toFixed(2)} · {site.kind === "Crooked" ? "crooked" : "legit"}
              </span>
            </div>
          ) : (
            <div class="wp-row" style={{ gap: "0.6em", flexWrap: "wrap", marginBottom: "0.5em" }}>
              <label class="wp-mono wp-dim">
                district{" "}
                <select class="wp-select" value={bareDistrict} onChange={(e) => setDistrict((e.target as HTMLSelectElement).value)} data-testid="repair-district">
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
          )}
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

const WHEELS = ["Front left", "Front right", "Rear left", "Rear right"];

/** Tyres: tread per wheel and replacement. Repair All never fits tyres. */
function TiresPanel(props: { car: CarView; view: CareerView; api: CareerApi; run: (f: () => CareerView) => void }) {
  const { car, view, api } = props;
  const t = car.tires;
  const [picked, setPicked] = useState<boolean[]>(t.worn_out.slice());
  if (car.location !== "Garage") return null;
  const n = picked.filter(Boolean).length;
  const cost = n * t.price_each;
  return (
    <Panel title="Tyres" tag={t.worn_out.some(Boolean) ? "need replacing" : "ok"}>
      <table class="wp-table" data-testid="tires">
        <tbody>
          {WHEELS.map((w, i) => (
            <tr key={w}>
              <td>
                <input type="checkbox" checked={picked[i]} onChange={() => setPicked(picked.map((p, j) => (j === i ? !p : p)))} />
              </td>
              <td>{w}</td>
              <td style={{ width: "40%" }}>
                <Meter value={t.tread[i]} color={severityColor(1 - t.tread[i])} label="Tread" />
              </td>
              <td class="num">{t.flat[i] ? <span class="wp-danger">flat</span> : t.worn_out[i] ? <span class="wp-warn">bald</span> : `grip ${pct(t.grip[i])}`}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <div class="wp-row" style={{ gap: "0.5em", marginTop: "0.5em" }}>
        <Button variant="primary" disabled={n === 0 || cost > view.cash} onClick={() => props.run(() => api.replaceTires(car.uid, picked))}>
          Replace {n} {n === 1 ? "tyre" : "tyres"} <Money value={cost} />
        </Button>
        <span class="wp-dim" style={{ fontSize: "0.8em" }}>
          <Money value={t.price_each} /> each, fitted. Repairs never replace tyres.
        </span>
      </div>
    </Panel>
  );
}

/** Inline "are you sure" row for a destructive action (sell, scrap, big buys): the kit's, with the garage's test id. */
function Confirm(props: { text: ComponentChildren; yes: string; onYes: () => void; onNo: () => void }) {
  return <ConfirmRow {...props} testid="career-confirm" />;
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

function CarsTab(props: { view: CareerView; api: CareerApi; at: string | null; run: (f: () => CareerView) => void }) {
  const { view, api } = props;
  const [sel, setSel] = useState<number | null>(view.active_car);
  const [tuning, setTuning] = useState(false);
  const [sellAsk, setSellAsk] = useState(false);
  const car = view.cars.find((c) => c.uid === sel) ?? view.cars.find((c) => c.active) ?? view.cars[0];
  if (!car) {
    return (
      <Panel title="No cars">
        <p>Buy one at a car lot{view.floor_car ? ", or take the street beater" : ""}.</p>
        {view.floor_car && (
          <Button variant="primary" onClick={() => props.run(() => api.floorCar())}>
            Street beater (<Money value={view.floor_car[1]} />)
          </Button>
        )}
      </Panel>
    );
  }
  if (tuning && car.location === "Garage") return <TunePage car={car} api={api} run={props.run} onBack={() => setTuning(false)} />;
  return (
    <div class="wp-cg-cars">
      <div class="wp-cg-list" role="listbox" aria-label="Your cars">
        {view.cars.map((c) => (
          <CarCard key={c.uid} car={c} selected={c.uid === car.uid} onSelect={() => (setSel(c.uid), setSellAsk(false))} />
        ))}
      </div>
      <div class="wp-cg-detail">
        <Panel title={car.name} tag={car.rare ? `RARE · ${words(car.class)}` : words(car.class)}>
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
              <Button disabled={car.location !== "Garage"} onClick={() => api.workshop!(car.uid)} title="work on the car: take parts off, fit new ones, buy upgrades">
                Workshop
              </Button>
            )}
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
        <RepairPanel key={`r${car.uid}`} car={car} view={view} api={api} at={props.at} run={props.run} />
        <TiresPanel key={`t${car.uid}`} car={car} view={view} api={api} run={props.run} />
        <InsurancePanel car={car} view={view} api={api} run={props.run} />
      </div>
    </div>
  );
}

const LOT_KIND: Record<LotKind, string> = {
  Dealership: "Dealership",
  UsedLot: "Used cars",
  Junkyard: "Junkyard",
};

const LOT_BLURB: Record<LotKind, string> = {
  Dealership: "Brand new cars. The floor changes every day: not every model is in.",
  UsedLot: "Running cars, some tired. Dents and worn parts repair in the garage; tyres are bought separately.",
  Junkyard: "Barely running or dead, sometimes missing panels. What's wrong under the hood is a surprise. Rare finds turn up.",
};

/** The car lots (dealerships by tier, used lots, junkyards): pick one and drive there. */
function LotsTab(props: { view: CareerView; api: CareerApi; run: (f: () => CareerView) => void }) {
  const { view, api } = props;
  const [peek, setPeek] = useState<string | null>(null);
  let stock: LotStockView | null = null;
  if (peek) {
    try {
      stock = api.lot(peek);
    } catch {
      stock = null;
    }
  }
  const kinds: LotKind[] = ["Dealership", "UsedLot", "Junkyard"];
  return (
    <div data-testid="lots">
      {view.floor_car && (
        <Panel title="Street beater" tag="economy floor">
          <p>
            No usable car? The yard always has a beater:{" "}
            <Button size="sm" variant="primary" onClick={() => props.run(() => api.floorCar())}>
              Street beater (<Money value={view.floor_car[1]} />)
            </Button>
          </p>
        </Panel>
      )}
      {kinds.map((k) => (
        <Panel key={k} title={LOT_KIND[k]} tag={`restocks daily · night ${view.day}`}>
          <p class="wp-dim" style={{ fontSize: "0.85em", marginTop: 0 }}>
            {LOT_BLURB[k]}
          </p>
          <table class="wp-table">
            <tbody>
              {view.lots
                .filter((l) => l.kind === k)
                .map((l) => (
                  <tr key={l.id} data-lot={l.id}>
                    <td>
                      <b>{l.name}</b> <span class="wp-dim wp-mono">{"★".repeat(l.tier)}</span>
                      <div class="wp-dim" style={{ fontSize: "0.8em" }}>
                        {words(l.district)} · {l.classes.map(words).join(", ")}
                      </div>
                    </td>
                    <td class="num">{l.in_stock} cars</td>
                    <td class="num">{l.from_price !== null ? <>from <Money value={l.from_price} /></> : <span class="wp-dim">empty today</span>}</td>
                    <td>
                      <div class="wp-row" style={{ gap: "0.3em" }}>
                        <Button size="sm" pressed={peek === l.id} onClick={() => setPeek(peek === l.id ? null : l.id)}>
                          List
                        </Button>
                        <Button size="sm" variant="primary" disabled={view.run_active} onClick={() => api.visitLot(l.id)} title="drive to the lot and walk the rows">
                          Visit
                        </Button>
                      </div>
                    </td>
                  </tr>
                ))}
            </tbody>
          </table>
          {stock && stock.lot.kind === k && (
            <table class="wp-table" data-testid="lot-stock">
              <tbody>
                {stock.cars.map((c) => (
                  <tr key={c.slot} style={{ opacity: c.sold ? 0.5 : 1 }}>
                    <td>
                      <b>{c.name}</b> {c.rare && <Badge tone="info">rare</Badge>} {c.sold && <Badge>sold</Badge>}{" "}
                      {c.locked && <Badge tone="dim">needs {c.unlock_tier}</Badge>}
                    </td>
                    <td class="wp-dim">{words(c.class)}</td>
                    <td class="num">{c.condition !== null ? pct(c.condition) : "?"}</td>
                    <td class="num">
                      <Money value={c.price} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </Panel>
      ))}
    </div>
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

/** Ledger line label; career / betting entries are `Adjustment`s with a `career:` / `bet:` note. */
function reasonLabel(l: CareerView["ledger"][number]): string {
  const note = (l.detail as { Adjustment?: { note?: string } } | null)?.Adjustment?.note;
  if (note) {
    const [scope, kind, raw] = note.split(":");
    const rest = raw ? raw.charAt(0).toUpperCase() + raw.slice(1) : "";
    if (scope === "career" && kind === "tier_reward") return `Tier bonus · ${rest}`;
    if (scope === "career" && kind === "tier") return `Tier · ${rest}`;
    if (scope === "career" && kind === "rep") return "Reputation";
    if (scope === "career" && kind === "credits") return "Career bonus";
    if (scope === "bet") return kind === "stake" ? "Bet placed" : "Bet won";
  }
  return REASON[l.reason] ?? words(l.reason);
}

/** Garage "Touge battles" tooltip: what the player's tier opens (the picker shows the rest). */
function tougeHint(view: CareerView): string {
  const f = view.progress.features.find((x) => x.feature === "touge_races");
  if (!f || f.unlocked) return "Touge picker: side bets, pink slips and bosses as your rep tier allows";
  return `Touge battles open at ${words(f.needs)} (${f.needed_rep.toLocaleString("en-US")} rep); the picker shows what each needs`;
}

/** Career feature → label. */
const FEATURE: Record<CareerFeature, string> = {
  city_races: "City street races",
  city_betting: "Betting on city races",
  touge_races: "Touge races",
  touge_betting: "Touge betting",
  boss_races: "Boss races",
  pink_slips: "Pink slips",
  street_betting: "Street racer challenges",
};

/** Bounty and heat: what the police hold against the career, and the informants who make it
 * go away (`career_pay_off_bounty`, `career_buy_informant`). */
function PoliceTab(props: { view: CareerView; api: CareerApi; run: (f: () => CareerView) => void }) {
  const { view, api } = props;
  const payoff = view.bounty_payoff ?? 0;
  const invite = view.high_stakes_bounty ?? 0;
  return (
    <>
      <Panel title="Bounty" tag={view.bounty > 0 ? "wanted" : "clean"}>
        <div class="wp-cg-hero">
          <Stat label="Bounty" value={view.bounty.toLocaleString("en-US")} tone={view.bounty > 0 ? "warn" : "ok"} />
          <Stat label="Bail if busted" value={<Money value={view.bail_if_busted ?? Math.round(view.bounty / 2)} />} />
          <Stat label="Carried heat" value={view.carried_heat.toFixed(0)} unit={view.carried_level !== "None" ? view.carried_level : undefined} />
        </div>
        <ul class="wp-dim" style={{ fontSize: "0.85em", margin: "0.4em 0 0.6em", paddingLeft: "1.2em" }}>
          <li>Escapes and races won while wanted raise it. Getting busted costs half of it as bail, then clears it.</li>
          <li>The police know it: the bigger it is, the sooner backup arrives and the more cars come.</li>
          <li>
            Illegal daytime meets are invite only{invite > 0 ? ` (bounty ${invite.toLocaleString("en-US")})` : ""}, and pay a bigger purse the bigger your bounty.
          </li>
        </ul>
        <Button
          variant="primary"
          disabled={view.bounty === 0 || view.cash < payoff}
          title={view.bounty === 0 ? "No bounty" : view.cash < payoff ? "Not enough cash" : undefined}
          testid="pay-off-bounty"
          onClick={() => props.run(() => api.payOffBounty!())}
        >
          Pay it off (<Money value={payoff} />)
        </Button>
        <span class="wp-dim" style={{ fontSize: "0.8em", marginLeft: "0.6em" }}>
          Clears the bounty. Dearer than bail, but no fine, impound or lost rep. Also closes the invite-only meets.
        </span>
      </Panel>
      {api.buyInformant && (view.informants?.length ?? 0) > 0 && (
        <Panel title="Informants" tag="heat">
          <p class="wp-dim" style={{ fontSize: "0.85em" }}>
            Pay to make the police forget the car: the heat the next run starts with drops.
          </p>
          <table class="wp-table" data-testid="informants">
            <tbody>
              {view.informants!.map((i) => (
                <tr key={i.id}>
                  <td>{words(i.id)}</td>
                  <td class="num">−{i.heat_reduction.toFixed(0)} heat</td>
                  <td class="num">
                    <Button
                      size="sm"
                      disabled={view.carried_heat <= 0 || view.cash < i.cost}
                      title={view.carried_heat <= 0 ? "The police are not looking for you" : view.cash < i.cost ? "Not enough cash" : undefined}
                      onClick={() => props.run(() => api.buyInformant!(i.id))}
                    >
                      Pay <Money value={i.cost} />
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Panel>
      )}
    </>
  );
}

function CareerTab({ view }: { view: CareerView }) {
  const p = view.progress;
  return (
    <Panel title="Reputation" tag={p.tier_name}>
      <div class="wp-cg-hero">
        <Stat label="Tier" value={p.tier_name} tone="ok" />
        <Stat label="Rep" value={p.rep.toLocaleString("en-US")} />
        <Stat label="Max bet · city" value={<Money value={p.max_bet_city} />} />
        <Stat label="Max bet · touge" value={<Money value={p.max_bet_touge} />} />
      </div>
      {p.next ? (
        <div data-testid="career-next">
          <Meter value={p.tier_fraction} label="Progress to next tier" />
          <p class="wp-dim">
            {p.next.rep_needed.toLocaleString("en-US")} rep to <b>{p.next.name}</b>
            {p.next.reward > 0 && (
              <>
                {" "}
                (bonus <Money value={p.next.reward} />)
              </>
            )}
          </p>
        </div>
      ) : (
        <p class="wp-ok">Top of the ladder.</p>
      )}
      <table class="wp-table" data-testid="career-unlocks">
        <tbody>
          {p.features.map((f) => (
            <tr key={f.feature}>
              <td>{FEATURE[f.feature] ?? words(f.feature)}</td>
              <td class="num">{f.unlocked ? <Badge tone="ok">open</Badge> : <Badge tone="dim">{`${f.needs} · ${f.needed_rep.toLocaleString("en-US")} rep`}</Badge>}</td>
            </tr>
          ))}
          {p.classes.map((c) => (
            <tr key={c.class}>
              <td>{words(c.class)} cars at the dealer</td>
              <td class="num">{c.unlocked ? <Badge tone="ok">open</Badge> : <Badge tone="dim">{c.needs}</Badge>}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <p class="wp-dim" style={{ fontSize: "0.8em" }}>
        Tiers follow your current rep: getting busted can drop you a tier and close what it opened. Each tier's bonus is paid once.
      </p>
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
              <td>{reasonLabel(l)}</td>
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
        Arcade: your car and AI racers take 20 % damage; repairs at half price. Simulation: one hard hit degrades, several end the car;
        repairs at half price. Extreme: one hard hit at speed is a total loss, and a total-loss bust means the car is gone.
      </p>
      <div class="wp-row" style={{ gap: "0.5em" }}>
        <Button size="sm" pressed={mode === "Arcade"} onClick={() => setMode("Arcade")}>
          Arcade
        </Button>
        <Button size="sm" pressed={mode === "Simulation"} onClick={() => setMode("Simulation")}>
          Simulation
        </Button>
        <Button size="sm" pressed={mode === "Extreme"} onClick={() => setMode("Extreme")}>
          Extreme
        </Button>
        <Button variant="danger" onClick={() => (props.run(() => props.api.newCareer(mode)), props.onDone())}>
          Start over in {mode}
        </Button>
        <Button onClick={props.onDone}>Cancel</Button>
      </div>
    </Panel>
  );
}

function GarageScreen(props: { api: CareerApi; initial: CareerView; message: string | null; at: string | null }) {
  const { api } = props;
  const [view, setView] = useState<CareerView>(props.initial);
  const [tab, setTab] = useState<Tab>(props.initial.cars.length ? "cars" : "lots");
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
  const [importAsk, setImportAsk] = useState(false);
  const importSave = () => {
    setImportAsk(false);
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
          <Badge tone="info" title="Rep tier">
            {view.progress.tier_name}
          </Badge>
          <span>BOUNTY {view.bounty.toLocaleString("en-US")}</span>
          {view.carried_level !== "None" && <Badge tone="warn">wanted {view.carried_level}</Badge>}
          <Button size="sm" onClick={api.close} hotkey="Esc">
            Close
          </Button>
        </div>
      </header>
      <div class="wp-row wp-cg-start" style={{ gap: "0.5em", flexWrap: "wrap", marginBottom: "0.8em" }}>
        <Button variant="primary" disabled={!view.can_start} onClick={() => api.startRun("chase")}>
          Night run
        </Button>
        <Button disabled={!view.can_start} onClick={() => api.startRun("free")}>
          Free drive
        </Button>
        {api.races && (
          <Button disabled={!view.can_start} onClick={() => api.races?.()} testid="career-races" title="Race picker: buy-in, purse, bets and rep go through this career">
            Street races
          </Button>
        )}
        {api.touge && (
          <Button disabled={!view.can_start} onClick={() => api.touge?.()} testid="career-touge" title={tougeHint(view)}>
            Touge battles
          </Button>
        )}
        {view.start_blocker && <span class="wp-warn">{view.start_blocker}</span>}
        <span style={{ marginLeft: "auto" }} />
        <Button size="sm" onClick={() => api.exportSave()}>
          Export save
        </Button>
        <Button size="sm" pressed={importAsk} onClick={() => setImportAsk(!importAsk)}>
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
      {importAsk && (
        <Confirm
          text={<>Load a save file? It replaces this career (cash <Money value={view.cash} />). Export first to keep it.</>}
          yes="Choose file"
          onYes={importSave}
          onNo={() => setImportAsk(false)}
        />
      )}
      {fresh && <NewCareer api={api} run={run} onDone={() => setFresh(false)} />}
      <Tabs
        tabs={[
          { id: "cars", label: `Cars (${view.cars.length})` },
          { id: "lots", label: "Car lots" },
          { id: "pound", label: pound ? `Impound (${pound})` : "Impound" },
          ...(api.payOffBounty ? [{ id: "police" as const, label: view.bounty > 0 ? "Police · bounty" : "Police" }] : []),
          { id: "career", label: "Career" },
          { id: "ledger", label: "Ledger" },
        ]}
        value={tab}
        onChange={setTab}
      />
      <div style={{ marginTop: "0.8em" }}>
        {tab === "cars" && <CarsTab view={view} api={api} at={props.at} run={run} />}
        {tab === "lots" && <LotsTab view={view} api={api} run={run} />}
        {tab === "pound" && <PoundTab view={view} api={api} run={run} />}
        {tab === "police" && <PoliceTab view={view} api={api} run={run} />}
        {tab === "career" && <CareerTab view={view} />}
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
   * could not start); `at` names the shop the car pulled into (the Repair panel starts there). */
  show(message: string | null = null, at: string | null = null): void {
    this.open = true;
    this.key++;
    const view = this.api.view();
    render(
      <div class="wp-ui wp-root" style={{ position: "absolute", inset: 0, pointerEvents: "auto", zIndex: 80 }}>
        <GarageScreen key={this.key} api={this.api} initial={view} message={message} at={at} />
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
