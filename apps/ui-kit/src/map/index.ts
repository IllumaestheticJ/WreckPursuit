/**
 * City map HUD: corner minimap + full city map (workstream 16). Data from `wp_wasm`
 * `map_static_json()` / `map_dynamic(player)` (see `data.ts`).
 */
export * from "./data";
export * from "./projection";
export { districtLabels, districtName, zoneAt, CITY_NAMES, type DistrictLabel } from "./districts";
export { MinimapRenderer, type MinimapOptions, type MinimapStats } from "./Minimap";
export { FullMapRenderer, type FullMapStats } from "./FullMap";
export { MapHud, mountMapHud, type MapHudOptions, type MapHudStats } from "./mapHud";
