/**
 * Live (sim-fed) entry point for hosts such as the browser testbed. Import from here rather than
 * `src/index.ts` to keep fixtures and mock data out of the host bundle.
 */
export { ChaseEndDetector, chaseEndOf, isChased, type ChaseEnd, type ChaseEndInfo } from "./chaseEnd";
export { createLiveModel, type LiveFrame, type LiveModel, type LiveOptions, type LiveVehicle, type PoliceReadout } from "./LiveModel";
export { mountOverlay, type OverlayScreen } from "./mount";
export { setBuildMeasurer, type BuildMeasurer } from "../data/measure";
export { billFromWasm, billRequestJson, DEFAULT_BILL_REQUEST, type LiveBillJson } from "./bill";
export { mountMapHud, MapHud, decodeMapDynamic, type MapHudOptions, type MapHudStats, type MapStatic, type MapFrame } from "../map";
export * from "../menu";
