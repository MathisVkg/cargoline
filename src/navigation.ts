import { Boxes, ClipboardList, History, Settings, type LucideIcon } from "lucide-react";

export type PageId = "contracts" | "cargo" | "history" | "settings";

export interface PageDefinition {
  id: PageId;
  label: string;
  icon: LucideIcon;
}

/** The four primary sections, in bottom-navigation order. */
export const PAGES: readonly PageDefinition[] = [
  { id: "contracts", label: "Contracts", icon: ClipboardList },
  { id: "cargo", label: "Cargo", icon: Boxes },
  { id: "history", label: "History", icon: History },
  { id: "settings", label: "Settings", icon: Settings },
];
