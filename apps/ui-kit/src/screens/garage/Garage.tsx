/**
 * Garage: car list, condition, the repair decision (repair all / choose items / drive damaged /
 * scrap) with an itemised bill, and insurance. Quotes come from `quoteRepair` (mock of
 * `wp_economy::Economy::quote_repair`); the purchase itself is a `garage.repair` command.
 */
import { useState } from "preact/hooks";
import type { BodyRegion, Component } from "../../generated/contracts";
import { useSend, useSlice } from "../../model/GameModel";
import type { GarageCar, RepairChoice } from "../../model/types";
import { INSURANCE_TIERS, insuranceQuote, quoteRepair, salePrice } from "../../data/economy";
import { MODS } from "../../data/catalog";
import { statSourceLabel, useBuildStats } from "../../data/measure";
import { DamageSilhouette } from "../hud/DamageSilhouette";
import { Badge, Button, CarStatBars, Meter, Money, Panel, ScreenHead, severityColor, Tabs, useEscape } from "../../ui/kit";
import { CarThumb } from "../../ui/CarThumb";
import { pct, words } from "../../ui/format";
import "./garage.css";

type Mode = "RepairAll" | "RepairSelected" | "DriveDamaged" | "Scrap";

/** Garage screen. */
export function Garage() {
  const send = useSend();
  const cars = useSlice((s) => s.garage.cars);
  const active = useSlice((s) => s.garage.activeCar);
  const [selected, setSelected] = useState<number | null>(active ?? cars[0]?.uid ?? null);
  useEscape(() => send({ type: "navigate", screen: "mainMenu" }));
  const car = cars.find((c) => c.uid === selected) ?? cars[0];
  return (
    <div class="wp-screen">
      <ScreenHead kicker="// GARAGE · BAY 3" title="Garage" back="mainMenu">
        <Button size="sm" onClick={() => send({ type: "navigate", screen: "market" })}>
          Market
        </Button>
      </ScreenHead>
      <div class="wp-garage">
        <div class="wp-garage-list" role="listbox" aria-label="Your cars">
          {cars.map((c) => (
            <CarCard key={c.uid} car={c} active={c.uid === active} selected={c.uid === car?.uid} onSelect={() => setSelected(c.uid)} />
          ))}
        </div>
        {car ? <CarDetail car={car} active={car.uid === active} /> : <Panel title="No cars">Buy one in the market.</Panel>}
      </div>
    </div>
  );
}

function CarCard(props: { car: GarageCar; active: boolean; selected: boolean; onSelect: () => void }) {
  const c = props.car;
  return (
    <button type="button" role="option" aria-selected={props.selected} class="wp-car-card" onClick={props.onSelect}>
      <div class="wp-spread">
        <span class="wp-car-name">{c.name}</span>
        <span class="wp-mono wp-dim">{words(c.class)}</span>
      </div>
      <Meter value={c.health} color={severityColor(1 - c.health)} label="Condition" />
      <div class="wp-row" style={{ gap: "0.3em" }}>
        {props.active && <Badge tone="ok">Active</Badge>}
        {c.totalLoss && <Badge tone="danger" solid>Total loss</Badge>}
        {c.location.kind === "Impound" && <Badge tone="danger">Impound</Badge>}
        {c.outstanding.lines.length > 0 && !c.totalLoss && <Badge tone="warn">{c.outstanding.lines.length} repairs</Badge>}
        {c.insurance && <Badge tone="info">{c.insurance.tier}</Badge>}
        {c.mods.length > 0 && <Badge>{c.mods.length} mods</Badge>}
      </div>
    </button>
  );
}

/** Region severities implied by the outstanding repair lines (for the silhouette). */
function regionsOf(car: GarageCar): { regions: Partial<Record<BodyRegion, number>>; detached: BodyRegion[]; components: Partial<Record<Component, number>> } {
  const regions: Partial<Record<BodyRegion, number>> = {};
  const detached: BodyRegion[] = [];
  const components: Partial<Record<Component, number>> = {};
  for (const l of car.outstanding.lines) {
    if ("Region" in l.item) regions[l.item.Region] = l.severity;
    else if ("DetachedPart" in l.item) detached.push(l.item.DetachedPart);
    else components[l.item.Component] = 1 - l.severity;
  }
  return { regions, detached, components };
}

function CarDetail({ car, active }: { car: GarageCar; active: boolean }) {
  const send = useSend();
  const dmg = regionsOf(car);
  const inImpound = car.location.kind === "Impound";
  return (
    <div class="wp-garage-detail">
      <Panel title={car.name} tag={words(car.class)} class="wp-garage-hero">
        <div class="wp-garage-hero-grid">
          <DamageSilhouette regions={dmg.regions} detached={dmg.detached} components={dmg.components} totalLoss={car.totalLoss} width="8em" labels />
          <div class="wp-stack">
            <CarThumb id={car.defId} size="lg" alt={car.name} />
            <div class="wp-grid" style={{ gridTemplateColumns: "repeat(3, auto)", justifyContent: "start", columnGap: "2em" }}>
              <div>
                <div class="wp-dim wp-mono wp-upper" style={{ fontSize: "0.68em" }}>Condition</div>
                <div class="wp-mono" style={{ fontSize: "1.6em", color: severityColor(1 - car.health) }}>{pct(car.health)}</div>
              </div>
              <div>
                <div class="wp-dim wp-mono wp-upper" style={{ fontSize: "0.68em" }}>Value</div>
                <div style={{ fontSize: "1.6em" }}>
                  <Money value={car.value} />
                </div>
              </div>
              <div>
                <div class="wp-dim wp-mono wp-upper" style={{ fontSize: "0.68em" }}>Dealer pays</div>
                <div style={{ fontSize: "1.6em" }}>
                  <Money value={salePrice(car)} />
                </div>
              </div>
            </div>
            <div class="wp-mono wp-dim2" style={{ fontSize: "0.85em" }}>
              {car.mods.length ? car.mods.map((m) => MODS[m]?.name ?? m).join(" · ") : "Stock"}
            </div>
            <BuildStatBars car={car} />
            {car.sustainedBeforeCapture && car.outstanding.lines.length > 0 && <div class="wp-warn">This car was caught by the police with this damage: insurance will not pay for it.</div>}
            <div class="wp-row">
              <Button variant="primary" disabled={active || !car.drivable || inImpound} onClick={() => send({ type: "garage.setActive", car: car.uid })}>
                {active ? "Active car" : "Drive this car"}
              </Button>
              <Button disabled={inImpound} onClick={() => send({ type: "navigate", screen: "modTree" })}>
                Mod tree
              </Button>
              {inImpound && <Button onClick={() => send({ type: "navigate", screen: "impound" })}>Go to impound</Button>}
            </div>
          </div>
        </div>
      </Panel>
      {!inImpound && <RepairPanel key={car.uid} car={car} />}
      {!inImpound && <InsurancePanel car={car} />}
    </div>
  );
}

/** The car's stat bars as built; with mods, a tick marks stock. */
function BuildStatBars({ car }: { car: GarageCar }) {
  const mode = useSlice((s) => s.settings.damageMode);
  const b = useBuildStats(car.defId, car.mods, mode, car.stats);
  if (!b) return null;
  const modded = car.mods.length > 0;
  return (
    <div class="wp-garage-stats" data-source={b.source} data-pending={b.pending}>
      <div class="wp-dim wp-mono wp-upper" style={{ fontSize: "0.68em" }}>
        Performance{modded ? ` · with mods (${statSourceLabel(b)}) · tick = stock` : " · stock"}
      </div>
      <CarStatBars stats={b.stats} baseline={modded ? b.stock : undefined} />
    </div>
  );
}

function RepairPanel({ car }: { car: GarageCar }) {
  const send = useSend();
  const shops = useSlice((s) => s.garage.shops);
  const shopId = useSlice((s) => s.garage.selectedShop);
  const cash = useSlice((s) => s.account.cash);
  const mode = useSlice((s) => s.settings.damageMode);
  const wanted = useSlice((s) => s.hud.heat.level !== "None");
  const [tab, setTab] = useState<Mode>(car.totalLoss ? "Scrap" : "RepairAll");
  const [picked, setPicked] = useState<number[]>([]);
  const shop = shops.find((s) => s.id === shopId) ?? shops[0];
  const choice: RepairChoice = tab === "RepairSelected" ? { kind: "RepairSelected", lines: picked } : { kind: tab };
  const q = quoteRepair(car, choice, shop, mode, cash, wanted);
  const allQ = quoteRepair(car, { kind: "RepairAll" }, shop, mode, cash, wanted);
  const nothing = car.outstanding.lines.length === 0;
  const toggle = (i: number) => setPicked((p) => (p.includes(i) ? p.filter((x) => x !== i) : [...p, i]));

  return (
    <Panel title="Repair decision" tag={`${mode.toUpperCase()} · ×${q.modeScale} BILL`}>
      {nothing ? (
        <p class="wp-ok" style={{ margin: 0 }}>
          Nothing to fix. Clean car.
        </p>
      ) : (
        <>
          <div class="wp-spread" style={{ marginBottom: "0.6em" }}>
            <Tabs<Mode>
              value={tab}
              onChange={setTab}
              tabs={[
                { id: "RepairAll", label: "Repair all" },
                { id: "RepairSelected", label: "Choose items" },
                { id: "DriveDamaged", label: "Drive damaged" },
                { id: "Scrap", label: "Scrap" },
              ]}
            />
            <label class="wp-row wp-mono" style={{ fontSize: "0.85em" }}>
              <span class="wp-dim">SHOP</span>
              <select class="wp-select" value={shop.id} onChange={(e) => send({ type: "garage.selectShop", shop: (e.target as HTMLSelectElement).value })}>
                {shops.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name}
                  </option>
                ))}
              </select>
              {shop.kind === "crooked" ? <Badge tone="warn">No reports{wanted ? " · ×1.6" : " · ×1.15"}</Badge> : <Badge tone="info">Reports L2+</Badge>}
            </label>
          </div>
          {tab === "DriveDamaged" && (
            <p class="wp-dim2">
              Free. The car keeps its bent metal and its handling effects: a car you could not afford to align drives crooked.
              {!car.drivable && <b class="wp-danger"> This car is a total loss and cannot be driven.</b>}
            </p>
          )}
          {tab === "Scrap" && (
            <p class="wp-dim2">
              Sell to the scrapyard for <Money value={q.scrapValue} tone /> (salvage + 30 % of installed mods). The car is gone for good.
            </p>
          )}
          {(tab === "RepairAll" || tab === "RepairSelected") && (
            <table class="wp-table wp-bill">
              <thead>
                <tr>
                  {tab === "RepairSelected" && <th />}
                  <th>Item</th>
                  <th class="num">Damage</th>
                  <th class="num">Parts</th>
                  <th class="num">Labour</th>
                  <th class="num">Total</th>
                </tr>
              </thead>
              <tbody>
                {allQ.lines.map((l, i) => {
                  const on = tab === "RepairAll" || picked.includes(i);
                  return (
                    <tr key={i} data-off={!on} onClick={tab === "RepairSelected" ? () => toggle(i) : undefined} data-click={tab === "RepairSelected" ? "" : undefined}>
                      {tab === "RepairSelected" && (
                        <td>
                          <input type="checkbox" checked={on} aria-label={`Repair ${l.label}`} onClick={(e) => e.stopPropagation()} onChange={() => toggle(i)} />
                        </td>
                      )}
                      <td>{l.label}</td>
                      <td class="num" style={{ color: severityColor(l.severity) }}>
                        {pct(l.severity)}
                      </td>
                      <td class="num">
                        <Money value={l.parts} />
                      </td>
                      <td class="num">
                        <span class="wp-dim">{l.labourHours.toFixed(1)} h · </span>
                        <Money value={l.labour} />
                      </td>
                      <td class="num">
                        <Money value={l.total} />
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
          <div class="wp-bill-foot">
            {(tab === "RepairAll" || tab === "RepairSelected") && (
              <>
                <div class="wp-spread">
                  <span class="wp-dim2">Subtotal ({q.lines.length} items, labour ×{shop.labourMultiplier.toFixed(2)} {shop.district})</span>
                  <Money value={q.subtotal} />
                </div>
                {car.insurance && (
                  <div class="wp-spread">
                    <span class="wp-dim2">
                      Insurance — {car.insurance.tier} ({pct(car.insurance.coverage)} after ${car.insurance.deductible} deductible)
                    </span>
                    {q.insuranceDenied ? <Badge tone="danger">Denied: damage before capture</Badge> : <Money value={-q.insuranceCovered} signed tone />}
                  </div>
                )}
              </>
            )}
            <div class="wp-spread wp-bill-total">
              <span>{q.total < 0 ? "You receive" : "You pay"}</span>
              <Money value={Math.abs(q.total)} tone={q.total < 0} />
            </div>
            <div class="wp-spread wp-mono wp-dim" style={{ fontSize: "0.8em" }}>
              <span>Cash after</span>
              <Money value={cash - q.total} />
            </div>
            <div class="wp-row" style={{ justifyContent: "flex-end" }}>
              {!q.affordable && <span class="wp-danger wp-mono">Not enough cash</span>}
              <Button
                variant={tab === "Scrap" ? "danger" : "primary"}
                disabled={!q.affordable || (tab === "RepairSelected" && picked.length === 0) || (tab === "DriveDamaged" && !car.drivable)}
                onClick={() => {
                  send({ type: "garage.repair", car: car.uid, choice });
                  setPicked([]);
                }}
              >
                {tab === "Scrap" ? "Scrap car" : tab === "DriveDamaged" ? "Drive it as-is" : `Pay $${Math.max(q.total, 0).toLocaleString("en-US")}`}
              </Button>
            </div>
          </div>
        </>
      )}
    </Panel>
  );
}

function InsurancePanel({ car }: { car: GarageCar }) {
  const send = useSend();
  return (
    <Panel title="Insurance" tag={car.insurance ? `${car.insurance.tier.toUpperCase()} · $${car.insurance.premiumPerDay}/DAY` : "UNINSURED"}>
      <div class="wp-grid" style={{ gridTemplateColumns: "repeat(2, minmax(0, 1fr))" }}>
        {Object.keys(INSURANCE_TIERS).map((tier) => {
          const q = insuranceQuote(car, tier);
          const current = car.insurance?.tier === tier;
          return (
            <div class="wp-ins-card" data-current={current} key={tier}>
              <div class="wp-spread">
                <b class="wp-upper">{tier}</b>
                <Money value={q.premiumPerDay} />
              </div>
              <div class="wp-mono wp-dim2" style={{ fontSize: "0.8em" }}>
                per day · covers {pct(q.coverage)} after ${q.deductible}
              </div>
              <Button size="sm" disabled={current} onClick={() => send({ type: "garage.buyInsurance", car: car.uid, tier })}>
                {current ? "Current policy" : car.insurance ? "Switch" : "Buy"}
              </Button>
            </div>
          );
        })}
      </div>
      <p class="wp-dim" style={{ fontSize: "0.8em", marginBottom: 0 }}>
        Cover holds during pursuits. Getting caught voids the policy, and damage on the car at the capture is never covered. An unpaid premium lapses it.
      </p>
    </Panel>
  );
}
