// Debug overlay: RTT, misprediction count, correction magnitude, bandwidth, lattice hashes.
import type { NetStats, NetStatus, UnitOrder } from "./wasm";

function orderText(o: UnitOrder): string {
  if (typeof o === "string") return o.toLowerCase();
  if ("Pursue" in o) return `pursue #${o.Pursue.target}`;
  if ("Intercept" in o) return `intercept #${o.Intercept.target}`;
  return `respond to incident ${o.Respond.incident}`;
}

/** `radio` = recent police radio lines (newest last), `traffic` = kinematic agents drawn. */
export function renderOverlay(
  el: HTMLElement,
  st: NetStatus,
  s: NetStats,
  conn: string,
  radio: string[] = [],
  traffic = 0,
): void {
  const kb = (b: number) => (b / 1000).toFixed(1);
  el.textContent =
    `net ${conn} · ${st.phase} · player ${st.player ?? "-"} · ${st.role} · ${st.mode}\n` +
    `rtt ${s.rtt_ms.toFixed(0)} ms ±${s.jitter_ms.toFixed(0)} · tick ${s.predicted_tick ?? "-"}\n` +
    `mispredictions ${s.mispredictions}/${s.reconciles} · correction last ${s.last_correction_m.toFixed(3)} m ` +
    `max ${s.max_correction_m.toFixed(3)} m rms ${s.correction_rms_m.toFixed(3)} m\n` +
    `down ${kb(s.bps_in)} kB/s · up ${kb(s.bps_out)} kB/s · remotes ${s.remotes} · traffic ${traffic}\n` +
    `lattice hashes ${s.hash_checks} mismatches ${s.hash_mismatches} resyncs ${s.resyncs}\n` +
    `cash ${st.account.cash} rep ${st.account.rep} cop rank ${st.account.cop_rank}` +
    (st.weather ? `\nweather ${st.weather.kind} ${(st.weather.intensity * 100).toFixed(0)} %` : "") +
    (st.wanted.length ? `\nwanted ${st.wanted.map((w) => `#${w.racer}★${w.level}`).join(" ")}` : "") +
    (st.unit_order ? `\norder: ${orderText(st.unit_order)}` : "") +
    radio.map((r) => `\nradio: ${r}`).join("") +
    (st.notices.length ? `\n> ${st.notices[st.notices.length - 1]}` : "");
}
