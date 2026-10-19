/** Market: buy cars (by condition) and sell owned ones. Prices follow `market.toml`. */
import { useMemo, useState } from "preact/hooks";
import { useSend, useSlice } from "../../model/GameModel";
import type { MarketListing } from "../../model/types";
import { salePrice } from "../../data/economy";
import { Badge, Button, ChoiceField, Money, Panel, ScreenHead, Tabs, useEscape } from "../../ui/kit";
import { pct, words } from "../../ui/format";
import "./garage.css";

type Cond = MarketListing["condition"];

/** Market screen. */
export function Market() {
  const send = useSend();
  const listings = useSlice((s) => s.market.listings);
  const cars = useSlice((s) => s.garage.cars);
  const cash = useSlice((s) => s.account.cash);
  const inDebt = useSlice((s) => s.account.cash < 0);
  const [tab, setTab] = useState<"buy" | "sell">("buy");
  const [cond, setCond] = useState<Cond>("Used");
  const [cls, setCls] = useState<string>("All");
  useEscape(() => send({ type: "back" }));
  const classes = useMemo(() => ["All", ...new Set(listings.map((l) => l.class))], [listings]);
  const rows = listings.filter((l) => l.condition === cond && (cls === "All" || l.class === cls));
  return (
    <div class="wp-screen">
      <ScreenHead kicker="// MARKET · CLASSIFIEDS" title="Market" back="back" />
      <Tabs
        value={tab}
        onChange={setTab}
        tabs={[
          { id: "buy", label: "Buy" },
          { id: "sell", label: `Sell (${cars.length})` },
        ]}
      />
      {inDebt && <p class="wp-danger">You are in debt: purchases are blocked until it is paid off.</p>}
      {tab === "buy" ? (
        <Panel title="For sale" tag={`${rows.length} CARS`}>
          <div class="wp-row" style={{ justifyContent: "space-between", marginBottom: "0.6em" }}>
            <ChoiceField<Cond>
              label="Condition"
              hint="New 100 % · Used 80 % (starts at 90 % health) · Worn 60 % (75 % health)"
              value={cond}
              options={[
                { id: "New", label: "New" },
                { id: "Used", label: "Used" },
                { id: "Worn", label: "Worn" },
              ]}
              onChange={setCond}
            />
            <label class="wp-row">
              <span class="wp-dim wp-mono">CLASS</span>
              <select class="wp-select" value={cls} onChange={(e) => setCls((e.target as HTMLSelectElement).value)}>
                {classes.map((c) => (
                  <option key={c} value={c}>
                    {words(c)}
                  </option>
                ))}
              </select>
            </label>
          </div>
          <table class="wp-table">
            <thead>
              <tr>
                <th>Car</th>
                <th>Class</th>
                <th class="num">Index</th>
                <th class="num">Price</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {rows.map((l) => (
                <tr key={`${l.defId}-${l.condition}`}>
                  <td>
                    <b>{l.name}</b>
                    <div class="wp-dim" style={{ fontSize: "0.8em" }}>
                      {l.analogOf}
                    </div>
                  </td>
                  <td>{words(l.class)}</td>
                  <td class="num">
                    <span class="wp-class-chip">{l.bracket}</span> {l.classIndex}
                  </td>
                  <td class="num">
                    <Money value={l.price} />
                  </td>
                  <td class="num">
                    <Button size="sm" variant="primary" disabled={l.price > cash || inDebt} onClick={() => send({ type: "market.buy", defId: l.defId, condition: l.condition })}>
                      Buy
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Panel>
      ) : (
        <Panel title="Your cars" tag="DEALER OFFERS">
          <table class="wp-table">
            <thead>
              <tr>
                <th>Car</th>
                <th class="num">Condition</th>
                <th class="num">Mods</th>
                <th class="num">Value</th>
                <th class="num">Offer</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {cars.map((c) => (
                <tr key={c.uid}>
                  <td>
                    <b>{c.name}</b> {c.location.kind === "Impound" && <Badge tone="danger">Impound</Badge>} {c.totalLoss && <Badge tone="danger">Total loss</Badge>}
                  </td>
                  <td class="num">{pct(c.health)}</td>
                  <td class="num">{c.mods.length}</td>
                  <td class="num">
                    <Money value={c.value} />
                  </td>
                  <td class="num">
                    <Money value={salePrice(c)} tone />
                  </td>
                  <td class="num">
                    <Button size="sm" variant="danger" disabled={c.location.kind !== "Garage" || cars.length === 1} onClick={() => send({ type: "market.sell", car: c.uid })}>
                      Sell
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <p class="wp-dim" style={{ fontSize: "0.8em" }}>
            Dealers pay 70 % of base price plus 40 % of mods, scaled by condition. A total loss sells for scrap.
          </p>
        </Panel>
      )}
    </div>
  );
}
