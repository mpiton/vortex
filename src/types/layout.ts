import type { LucideIcon } from 'lucide-react';
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
} from 'lucide-react';

export type Theme = 'light' | 'dark' | 'auto';

export interface RouteConfig {
  icon: LucideIcon;
  label: string;
  path: string;
  shortcut: string;
}

export const ROUTES: RouteConfig[] = [
  { icon: DownloadCloud, label: 'Downloads', path: '/downloads', shortcut: '⌘1' },
  { icon: Link2, label: 'Link Grabber', path: '/link-grabber', shortcut: '⌘2' },
  { icon: Package, label: 'Packages', path: '/packages', shortcut: '⌘3' },
  { icon: User, label: 'Accounts', path: '/accounts', shortcut: '⌘4' },
  { icon: Shield, label: 'Captcha', path: '/captcha', shortcut: '⌘5' },
  { icon: Puzzle, label: 'Plugins', path: '/plugins', shortcut: '⌘6' },
  { icon: Clock, label: 'Scheduler', path: '/scheduler', shortcut: '⌘7' },
  { icon: History, label: 'History', path: '/history', shortcut: '⌘8' },
  { icon: BarChart3, label: 'Statistics', path: '/statistics', shortcut: '⌘9' },
  { icon: Settings, label: 'Settings', path: '/settings', shortcut: '⌘,' },
];
