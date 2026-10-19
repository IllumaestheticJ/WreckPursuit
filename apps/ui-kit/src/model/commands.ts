/**
 * Every action a screen can request. The provider decides whether it is allowed (the economy is
 * server-authoritative) and updates [`UiState`]; refused commands set `UiState.notice`.
 *
 * `docs/integration/ui.md` maps each command to the crate / function that will execute it.
 */
import type { DamageMode, Role } from "../generated/contracts";
import type { InputAction, InputDevice } from "./bindings";
import type { CaptainCommand } from "./police";
import type { AssistPreset, BillRequest, RepairChoice, ReplayState, ScreenId, Settings } from "./types";

/** Deep-partial settings patch. */
export type SettingsPatch = {
  [K in keyof Settings]?: Settings[K] extends object ? Partial<Settings[K]> : Settings[K];
};

/** Every UI command. */
export type UiCommand =
  // navigation
  | { type: "navigate"; screen: ScreenId }
  | { type: "back" }
  | { type: "pause" }
  | { type: "resume" }
  | { type: "quitToMenu" }
  // settings
  | { type: "settings.patch"; patch: SettingsPatch }
  | { type: "settings.assistPreset"; preset: AssistPreset }
  | { type: "settings.damageMode"; mode: DamageMode }
  | { type: "settings.bind"; action: InputAction; device: InputDevice; code: string }
  | { type: "settings.unbind"; action: InputAction; device: InputDevice }
  | { type: "settings.resetBindings" }
  // garage
  | { type: "garage.setActive"; car: number }
  | { type: "garage.selectShop"; shop: string }
  | { type: "garage.repair"; car: number; choice: RepairChoice }
  | { type: "garage.buyInsurance"; car: number; tier: string }
  | { type: "mods.install"; car: number; mod: string }
  | { type: "mods.uninstall"; car: number; mod: string }
  // market
  | { type: "market.buy"; defId: string; condition: "New" | "Used" | "Worn" }
  | { type: "market.sell"; car: number }
  // lobby / results
  | { type: "lobby.setRole"; role: Role }
  | { type: "lobby.setReady"; ready: boolean }
  | { type: "lobby.leave" }
  | { type: "results.continue" }
  // impound
  | { type: "impound.retrieve"; car: number }
  | { type: "impound.payFines" }
  // police
  | { type: "captain"; command: CaptainCommand }
  // replay
  | { type: "replay.play" }
  | { type: "replay.pause" }
  | { type: "replay.seek"; tick: number }
  | { type: "replay.step"; ticks: number }
  | { type: "replay.speed"; speed: number }
  | { type: "replay.camera"; camera: ReplayState["camera"] }
  | { type: "replay.load"; source: string }
  // repair bill (live car)
  | { type: "bill.request"; request: Partial<BillRequest> }
  // misc
  | { type: "notice.dismiss" };
