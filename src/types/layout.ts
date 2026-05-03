import type { LucideIcon } from "lucide-react";
import {
  DownloadCloud,
  Link2,
  Package,
  User,
  Shield,
  Puzzle,
  Clock,
  History,
  BarChart3,
  Settings,
} from "lucide-react";

export type Theme = "light" | "dark" | "auto";

export interface RouteConfig {
  icon: LucideIcon;
  labelKey: string;
  path: string;
  shortcut: string;
}

export const ROUTES: RouteConfig[] = [
  { icon: DownloadCloud, labelKey: "nav.downloads", path: "/downloads", shortcut: "⌘1" },
  { icon: Link2, labelKey: "nav.linkGrabber", path: "/link-grabber", shortcut: "⌘2" },
  { icon: Package, labelKey: "nav.packages", path: "/packages", shortcut: "⌘3" },
  { icon: User, labelKey: "nav.accounts", path: "/accounts", shortcut: "⌘4" },
  { icon: Shield, labelKey: "nav.captcha", path: "/captcha", shortcut: "⌘5" },
  { icon: Puzzle, labelKey: "nav.plugins", path: "/plugins", shortcut: "⌘6" },
  { icon: Clock, labelKey: "nav.scheduler", path: "/scheduler", shortcut: "⌘7" },
  { icon: History, labelKey: "nav.history", path: "/history", shortcut: "⌘8" },
  { icon: BarChart3, labelKey: "nav.statistics", path: "/statistics", shortcut: "⌘9" },
  { icon: Settings, labelKey: "nav.settings", path: "/settings", shortcut: "⌘," },
];
