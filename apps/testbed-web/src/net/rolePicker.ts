// Role picker + captain-offer prompt + race lobby panel.
import * as net from "./wasm";
import type { NetStatus, RenderVehicle, RoleName } from "./wasm";

const ROLES: [RoleName, string][] = [
  ["racer", "Racer"],
  ["police_chase", "Police unit"],
  ["police_captain", "Police captain"],
  ["spectator", "Spectator"],
];

export class LobbyPanel {
  private roleRow = document.createElement("div");
  private offer = document.createElement("div");
  private races = document.createElement("div");
  private lastKey = "";

  constructor(private root: HTMLElement, private own: () => RenderVehicle | undefined) {
    root.append(this.roleRow, this.offer, this.races);
    for (const [role, label] of ROLES) {
      const b = document.createElement("button");
      b.textContent = label;
      b.dataset.role = role;
      b.onclick = () => net.requestRole(role, performance.now());
      this.roleRow.append(b);
    }
  }

  update(st: NetStatus): void {
    this.roleRow.querySelectorAll("button").forEach((b) => {
      b.classList.toggle("active", b.dataset.role === st.role);
    });
    // Re-render the rest only when it changes (keeps buttons clickable).
    const key = JSON.stringify([st.captain_offer, st.races, st.race_results, st.role, st.player]);
    if (key === this.lastKey) return;
    this.lastKey = key;

    this.offer.replaceChildren();
    if (st.captain_offer !== null) {
      const b = document.createElement("button");
      b.textContent = "Accept captain seat";
      b.onclick = () => net.requestRole("police_captain", performance.now());
      this.offer.append("The captain seat is offered to you. ", b);
    }

    this.races.replaceChildren();
    if (st.role === "racer") {
      const create = document.createElement("button");
      create.textContent = "Create race ahead of me";
      create.onclick = () => this.createRaceAhead();
      this.races.append(create);
    }
    for (const r of st.races) {
      const row = document.createElement("div");
      row.textContent = `race ${r.race} (${r.state}) host ${r.host} stake ${r.stake} ` +
        `members ${r.members.join(",")} `;
      const member = st.player !== null && r.members.includes(st.player);
      if (!member && r.state === "lobby" && st.role === "racer") {
        row.append(button("Join", () => net.raceJoin(r.race, performance.now())));
      }
      if (member && r.host === st.player && r.state === "lobby") {
        row.append(button("Start", () => net.raceStart(r.race, performance.now())));
      }
      if (member) row.append(button("Leave", () => net.raceLeave(r.race, performance.now())));
      this.races.append(row);
    }
    if (st.race_results) {
      const res = document.createElement("div");
      res.textContent = `results race ${st.race_results.race}: ` + st.race_results.entries
        .map((e) => `${e.place ? "#" + e.place : "DNF"} p${e.player} ${e.time_s.toFixed(1)}s +${e.payout}`)
        .join(" · ");
      this.races.append(res);
    }
  }

  /** Three checkpoints 150 m apart along the own car's heading. */
  private createRaceAhead(): void {
    const me = this.own();
    if (!me) return;
    const [x, y, z, w] = me.rot;
    // Forward (+Y) rotated by the quaternion.
    const fx = 2 * (x * y - w * z);
    const fy = 1 - 2 * (x * x + z * z);
    const cps: [number, number][] = [1, 2, 3].map((k) => [me.pos[0] + fx * 150 * k, me.pos[1] + fy * 150 * k]);
    net.raceCreate(1, cps, performance.now());
  }
}

function button(label: string, onclick: () => void): HTMLButtonElement {
  const b = document.createElement("button");
  b.textContent = label;
  b.onclick = onclick;
  return b;
}
