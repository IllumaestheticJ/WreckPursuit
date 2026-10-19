/** Impound: cars in the police pound, growing fees, auction countdown, last capture summary. */
import { useSend, useSlice } from "../../model/GameModel";
import { Badge, Button, Meter, Money, Panel, ScreenHead, useEscape } from "../../ui/kit";
import { IMPOUND } from "../../data/economy";
import "../race/race.css";

/** Impound screen. */
export function Impound() {
  const send = useSend();
  const imp = useSlice((s) => s.impound);
  const cash = useSlice((s) => s.account.cash);
  useEscape(() => send({ type: "back" }));
  const cap = imp.lastCapture;
  return (
    <div class="wp-screen">
      <ScreenHead kicker="// CITY POUND · LOT 7" title="Impound" back="back" />
      <div class="wp-grid" style={{ gridTemplateColumns: "minmax(0, 1.5fr) minmax(0, 1fr)", alignItems: "start" }}>
        <Panel title="Your cars in the pound" tag={`${imp.entries.length}`}>
          {imp.entries.length === 0 && <p class="wp-ok">Nothing impounded. Keep it that way.</p>}
          {imp.entries.map((e) => (
            <div class="wp-impound-row" key={e.carUid}>
              <div>
                <div class="wp-car-name">{e.name}</div>
                <div class="wp-mono wp-dim" style={{ fontSize: "0.8em" }}>
                  {e.reason} · day {e.daysIn} in the pound
                </div>
              </div>
              <div>
                <div class="wp-spread wp-mono" style={{ fontSize: "0.8em" }}>
                  <span class="wp-dim">AUCTION IN</span>
                  <span class={e.auctionInDays <= 3 ? "wp-danger" : "wp-warn"}>{e.auctionInDays} days</span>
                </div>
                <Meter value={e.daysIn / IMPOUND.auctionAfterDays} color={e.auctionInDays <= 3 ? "var(--danger)" : "var(--warn)"} segmented label="Days until auction" />
              </div>
              <div class="wp-stack" style={{ textAlign: "right" }}>
                <div>
                  <Money value={e.fee} /> <span class="wp-dim wp-mono" style={{ fontSize: "0.8em" }}>+${e.feePerDay}/day</span>
                </div>
                <Button size="sm" variant="primary" disabled={e.fee > cash || cash < 0} onClick={() => send({ type: "impound.retrieve", car: e.carUid })}>
                  Pay & release
                </Button>
              </div>
            </div>
          ))}
          <p class="wp-dim" style={{ fontSize: "0.8em" }}>
            Fees grow every in-game day. After {IMPOUND.auctionAfterDays} days the car is auctioned and you get 10 % of its value. In debt you cannot retrieve cars.
          </p>
        </Panel>
        {cap && (
          <Panel title="Last capture" tag={`WANTED ${cap.level}`}>
            <dl class="wp-kv">
              <dt>Fine</dt>
              <dd>
                <Money value={-cap.fine} signed tone />
              </dd>
              <dt>Bail</dt>
              <dd>
                <Money value={-cap.bail} signed tone />
              </dd>
              <dt>Reputation</dt>
              <dd class="wp-danger">−{cap.repLost}</dd>
              <dt>Car</dt>
              <dd>
                <Badge tone="danger">{cap.fate}</Badge>
              </dd>
              <dt>Bounty</dt>
              <dd>cleared</dd>
            </dl>
            {imp.finesOwed > 0 && (
              <Button variant="danger" onClick={() => send({ type: "impound.payFines" })}>
                Pay fines (<Money value={imp.finesOwed} />)
              </Button>
            )}
          </Panel>
        )}
      </div>
    </div>
  );
}
