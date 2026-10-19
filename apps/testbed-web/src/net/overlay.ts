// Debug overlay: RTT, misprediction count, correction magnitude, bandwidth, lattice hashes.
import type { NetStats, NetStatus } from "./wasm";

export function renderOverlay(el: HTMLElement, st: NetStatus, s: NetStats, conn: string): void {
  const kb = (b: number) => (b / 1000).toFixed(1);
  el.textContent =
    `net ${conn} · ${st.phase} · player ${st.player ?? "-"} · ${st.role} · ${st.mode}\n` +
    `rtt ${s.rtt_ms.toFixed(0)} ms ±${s.jitter_ms.toFixed(0)} · tick ${s.predicted_tick ?? "-"}\n` +
    `mispredictions ${s.mispredictions}/${s.reconciles} · correction last ${s.last_correction_m.toFixed(3)} m ` +
    `max ${s.max_correction_m.toFixed(3)} m rms ${s.correction_rms_m.toFixed(3)} m\n` +
    `down ${kb(s.bps_in)} kB/s · up ${kb(s.bps_out)} kB/s · remotes ${s.remotes}\n` +
    `lattice hashes ${s.hash_checks} mismatches ${s.hash_mismatches} resyncs ${s.resyncs}\n` +
    `cash ${st.account.cash} rep ${st.account.rep} cop rank ${st.account.cop_rank}` +
    (st.unit_order ? `\norder: ${["", "move", "block", "pursue", "hold"][st.unit_order.kind]} ` +
      `(${st.unit_order.x.toFixed(0)}, ${st.unit_order.y.toFixed(0)}) target ${st.unit_order.target}` : "") +
    (st.notices.length ? `\n> ${st.notices[st.notices.length - 1]}` : "");
}
