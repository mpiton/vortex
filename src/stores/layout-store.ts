import { create } from 'zustand';

interface LayoutState {
  sidebarCollapsed: boolean;
  toggleSidebar: () => void;
  totalSpeed: number;
  speedLimit: number;
  freeSpace: string;
  activeCount: number;
  totalConnections: number;
  appVersion: string;
}

export const useLayoutStore = create<LayoutState>((set) => ({
  sidebarCollapsed: false,
  toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
  totalSpeed: 0,
  speedLimit: 0,
  freeSpace: '-- GB',
  activeCount: 0,
  totalConnections: 0,
  appVersion: '0.1.0',
}));
