// Mod shop: install mods on the selected car and see them on it.
//
// The list is the car's catalog (`mod_catalog_json`, class-filtered) grouped by slot and line;
// one mod per line, picking another tier replaces it. Every change is checked by the economy
// (`build_visuals_json`: requirements, class limits) and then applied by respawning the free
// drive with the set as the player's scenario `mods`, so the physics (wp_economy::apply_mods)
// and the parts drawn on the car (render/mod_parts.ts) always match. Builds are remembered per
// car in localStorage (`wp.builds`); `?mods=a,b` overrides it for automation.
import { sim, type ModEntry } from "../wasm/sim";
import { store } from "../settings";

const BUILDS_KEY = "wp.builds";

/** Slot display order (wp_economy::ModSlot). */
const SLOTS = [
  "Wheels",
  "Tires",
  "Brakes",
  "Suspension",
  "Aero",
  "BodyFront",
  "BodySides",
  "BodyRear",
  "Hood",
  "Roof",
  "Paint",
  "Vinyls",
  "Interior",
  "Protection",
  "Weight",
  "Engine",
  "ForcedInduction",
  "Cooling",
  "Drivetrain",
  "Electronics",
];

/** Saved builds by car id. */
function loadBuilds(): Record<string, string[]> {
  try {
    return JSON.parse(store.get(BUILDS_KEY) ?? "{}") as Record<string, string[]>;
  } catch {
    return {};
  }
}

const partLabel = (parts: [string, string][]) =>
  parts.length ? parts.map(([k, s]) => `${k} ${s.replace(/_/g, " ")}`).join(", ") : "not visible (internal)";

export class ModShop {
  private car = "";
  private catalog: ModEntry[] = [];
  private mods: string[] = [];
  private readonly list: HTMLElement;
  private readonly status: HTMLElement;
  private readonly fitted: HTMLElement;
  private readonly title: HTMLElement;

  /** `apply(mods)` restarts the session with the build on the player car. */
  constructor(
    readonly el: HTMLElement,
    private readonly apply: (mods: string[]) => void,
    private readonly onToggle: (open: boolean) => void,
  ) {
    el.classList.add("mod-shop", "panel", "hidden");
    const head = document.createElement("div");
    head.className = "mod-shop-head";
    this.title = document.createElement("b");
    const clear = document.createElement("button");
    clear.textContent = "stock";
    clear.title = "remove every mod";
    clear.onclick = () => this.set([]);
    const close = document.createElement("button");
    close.textContent = "×";
    close.title = "close";
    close.onclick = () => this.toggle(false);
    head.append(this.title, clear, close);
    this.fitted = document.createElement("div");
    this.fitted.className = "mod-shop-fitted dim";
    this.status = document.createElement("div");
    this.status.className = "mod-shop-status";
    this.list = document.createElement("div");
    this.list.className = "mod-shop-list";
    el.append(head, this.fitted, this.status, this.list);
  }

  get open(): boolean {
    return !this.el.classList.contains("hidden");
  }

  toggle(open = !this.open): void {
    this.el.classList.toggle("hidden", !open);
    this.onToggle(open);
  }

  /** Installed mods for `car` (URL override, else the saved build; dropped if no longer legal). */
  modsFor(car: string, urlMods: string[] | null): string[] {
    const want = urlMods ?? loadBuilds()[car] ?? [];
    if (want.length === 0) return [];
    try {
      sim.buildVisuals(car, want);
      return want;
    } catch (e) {
      console.warn(`saved build for ${car} is not legal any more, driving stock`, e);
      return [];
    }
  }

  /** Show the shop for the session's player car and its installed mods. */
  show(car: string, mods: string[]): void {
    if (car !== this.car) {
      this.car = car;
      try {
        this.catalog = sim.modCatalog(car);
      } catch (e) {
        this.catalog = [];
        console.warn("mod catalog unavailable", e);
      }
    }
    this.mods = [...mods];
    this.title.textContent = `mods · ${car}`;
    this.render();
  }

  /** Try a new set: refused with the economy's reason, otherwise saved and applied. */
  private set(next: string[]): void {
    try {
      sim.buildVisuals(this.car, next);
    } catch (e) {
      this.status.textContent = String(e).replace(/^.*wp_wasm: /, "");
      this.status.classList.add("bad");
      return;
    }
    this.status.textContent = "";
    this.status.classList.remove("bad");
    this.mods = next;
    const builds = loadBuilds();
    if (next.length) builds[this.car] = next;
    else delete builds[this.car];
    store.set(BUILDS_KEY, JSON.stringify(builds));
    this.apply(next);
  }

  private click(m: ModEntry): void {
    if (this.mods.includes(m.id)) return this.set(this.mods.filter((id) => id !== m.id));
    // One mod per line: a new tier replaces the installed one.
    const line = new Set(this.catalog.filter((c) => c.line === m.line && c.slot === m.slot).map((c) => c.id));
    this.set([...this.mods.filter((id) => !line.has(id)), m.id]);
  }

  private render(): void {
    const byId = new Map(this.catalog.map((m) => [m.id, m]));
    const visible = this.mods.map((id) => byId.get(id)).filter((m): m is ModEntry => !!m && m.parts.length > 0);
    this.fitted.textContent = this.mods.length
      ? `on the car: ${visible.map((m) => partLabel(m.parts)).join(" · ") || "(nothing visible)"}`
      : "stock car";
    this.list.textContent = "";
    for (const slot of SLOTS) {
      const mods = this.catalog.filter((m) => m.slot === slot);
      if (mods.length === 0) continue;
      const group = document.createElement("div");
      group.className = "mod-slot";
      const h = document.createElement("div");
      h.className = "mod-slot-name";
      h.textContent = slot;
      group.append(h);
      for (const m of mods.sort((a, b) => a.line.localeCompare(b.line) || a.tier - b.tier)) {
        const b = document.createElement("button");
        const on = this.mods.includes(m.id);
        b.className = `mod${on ? " on" : ""}${m.parts.length ? " shows" : ""}`;
        b.dataset.mod = m.id;
        b.textContent = `${on ? "✓ " : ""}${m.name}`;
        b.title = `${m.name} (tier ${m.tier}, $${m.price})\n${m.tradeoff}\nlooks: ${partLabel(m.parts)}${m.requires?.length ? `\nneeds: ${m.requires.join(", ")}` : ""}`;
        b.onclick = () => this.click(m);
        group.append(b);
      }
      this.list.append(group);
    }
  }
}
