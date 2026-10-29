// Workshop sounds: wraps the workshop api the overlay calls so every action that went through
// (the wasm call returned instead of throwing) plays its garage sound (audio/garage_sfx.ts).
// The overlay and the actions themselves are unchanged; the paint shop is wrapped from outside
// (its purchases sound like the spray gun) without touching how it paints.
import type { WorkshopApi } from "../../../ui-kit/src/garage/types";
import type { GarageSound } from "../audio/garage_sfx";

/** `api` with a sound after each successful action. */
export function withSounds(api: WorkshopApi, play: (s: GarageSound) => void): WorkshopApi {
  const after =
    <A extends unknown[], R>(f: (...a: A) => R, s: GarageSound) =>
    (...a: A): R => {
      const r = f(...a);
      play(s);
      return r;
    };
  const out: WorkshopApi = {
    ...api,
    buy: after(api.buy, "buy"),
    sell: after(api.sell, "buy"),
    remove: after(api.remove, "remove"),
    fit: after(api.fit, "install"),
    strip: after(api.strip, "remove"),
    build: after(api.build, "install"),
    clearWay: after(api.clearWay, "remove"),
    mechanic: (slot, source, sellOld) => {
      const r = api.mechanic(slot, source, sellOld);
      if (r.job.bought > 0) play("buy");
      play("fit");
      return r;
    },
  };
  if (api.install) out.install = after(api.install, "install");
  if (api.uninstall) out.uninstall = after(api.uninstall, "remove");
  const paint = api.paintShop;
  if (paint) out.paintShop = { ...paint, respray: after(paint.respray, "paint") };
  return out;
}
