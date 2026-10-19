/**
 * @wreckpursuit/ui-kit — public API.
 *
 * - `mountUi(el, { model })` renders the kit; `createMockModel()` is a complete mock data source.
 * - `GameModel` is the one interface a real provider (wp_wasm / wp_net bridge) implements.
 * - Individual screens and primitives are exported for hosts that compose their own layout.
 */
export { mountUi, type MountOptions, type UiHandle } from "./mount";
export { App, type AppProps } from "./App";
export { Store, ModelContext, useModel, useSlice, useSend, type GameModel } from "./model/GameModel";
export type * from "./model/types";
export type { UiCommand, SettingsPatch } from "./model/commands";
export type * from "./model/proposed";
export { wantedLevelNumber, POLICE_KIND } from "./model/proposed";
export * as bindings from "./model/bindings";
export type * as contracts from "./generated/contracts";

export { createMockModel, type MockModel, type MockOptions } from "./mock/MockProvider";
export { FIXTURES, loadFixture } from "./mock/fixtures";
export { parseEventsJsonl, parseMetricsCsv, parseRun, type EventLine, type MetricsRow, type Run } from "./mock/runs";
export { applyEvent, emptyPlayback, type Playback } from "./mock/reduce";

export { SCREENS, type ScreenEntry } from "./screens/registry";
export { Hud } from "./screens/hud/Hud";
export { DamageSilhouette } from "./screens/hud/DamageSilhouette";
export { Minimap } from "./screens/hud/Minimap";
export { ReplayControls } from "./screens/replay/ReplayScreen";
export { CaptainMap } from "./screens/captain/CaptainMap";
