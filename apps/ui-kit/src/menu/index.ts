/** Game menu: main / pause / settings for players (keyboard + gamepad). See GameMenu.tsx. */
export {
  DEFAULT_KEY_HINTS,
  GameMenu,
  mountGameMenu,
  type CareerSummary,
  type CityChoice,
  type GameMenuHost,
  type KeyHint,
  type GameMenuOptions,
  type MenuScreen,
  type PlayChoice,
  type PlayOptions,
} from "./GameMenu";
export {
  DRAW_DISTANCES,
  SETTINGS_KEY,
  defaultGameSettings,
  hasStoredSettings,
  loadGameSettings,
  normalizeSettings,
  saveGameSettings,
  speedIn,
  unitLabel,
  type AssistSettings,
  type GameSettings,
  type GfxLevel,
  type HudPreset,
  type SettingsStorage,
  type SpeedUnits,
  type Volumes,
} from "./settings";
export { keyAction, PadNav, stepIndex, type NavAction } from "./nav";
export { BAR_CHANNELS, paintInputBars, type BarChannel } from "./bars";
export {
  CONTROL_DEVICES,
  CONTROL_RANGES,
  defaultControls,
  defaultDeviceControls,
  normalizeControls,
  normalizeDeviceControls,
  shapePedal,
  shapePedals,
  shapeSteer,
  type ControlDevice,
  type DeviceControls,
  type InputProbe,
} from "./controls";
