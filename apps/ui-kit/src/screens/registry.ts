/** Every screen, with its demo-page label and group. */
import type { FunctionComponent } from "preact";
import type { ScreenId } from "../model/types";
import { CaptainMap } from "./captain/CaptainMap";
import { CrewMap } from "./crew/CrewMap";
import { Garage } from "./garage/Garage";
import { ModTree } from "./garage/ModTree";
import { Hud } from "./hud/Hud";
import { Impound } from "./impound/Impound";
import { Lobby } from "./race/Lobby";
import { Results } from "./race/Results";
import { MainMenu } from "./menu/MainMenu";
import { Market } from "./garage/Market";
import { Pause } from "./menu/Pause";
import { PoliceHome } from "./police/PoliceHome";
import { ReplayScreen } from "./replay/ReplayScreen";
import { ScannerFeed } from "./police/ScannerFeed";
import { Settings } from "./settings/Settings";

/** Screen registry entry. */
export interface ScreenEntry {
  id: ScreenId;
  label: string;
  group: "Race" | "Menus" | "Garage" | "Street" | "Police";
  component: FunctionComponent;
}

/** All screens in demo order. */
export const SCREENS: ScreenEntry[] = [
  { id: "hud", label: "HUD", group: "Race", component: Hud },
  { id: "replay", label: "Replay controls", group: "Race", component: ReplayScreen },
  { id: "mainMenu", label: "Main menu", group: "Menus", component: MainMenu },
  { id: "pause", label: "Pause", group: "Menus", component: Pause },
  { id: "settings", label: "Settings", group: "Menus", component: Settings },
  { id: "garage", label: "Garage", group: "Garage", component: Garage },
  { id: "modTree", label: "Mod tree", group: "Garage", component: ModTree },
  { id: "market", label: "Market", group: "Garage", component: Market },
  { id: "lobby", label: "Race lobby / meet", group: "Street", component: Lobby },
  { id: "results", label: "Results & payout", group: "Street", component: Results },
  { id: "impound", label: "Impound", group: "Street", component: Impound },
  { id: "crewMap", label: "Crews & territory", group: "Street", component: CrewMap },
  { id: "police", label: "Police career", group: "Police", component: PoliceHome },
  { id: "captain", label: "Captain map", group: "Police", component: CaptainMap },
  { id: "scanner", label: "Scanner feed", group: "Police", component: ScannerFeed },
];
