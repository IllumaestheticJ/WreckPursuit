/**
 * Live (sim-fed) entry point for hosts such as the browser testbed. Import from here rather than
 * `src/index.ts` to keep fixtures and mock data out of the host bundle.
 */
export { ChaseEndDetector, isChased, type ChaseEnd } from "./chaseEnd";
export { createLiveModel, type LiveFrame, type LiveModel, type LiveOptions, type LiveVehicle, type PoliceReadout } from "./LiveModel";
export { mountOverlay, type OverlayScreen } from "./mount";
export { billFromWasm, billRequestJson, DEFAULT_BILL_REQUEST, type LiveBillJson } from "./bill";
