import { create } from 'zustand';

interface LayoutState {
  sidebarCollapsed: boolean;
  toggleSidebar: () => void;
  speedLimit: number;
  freeSpace: string;
  appVersion: string;
}

export const useLayoutStore = create<LayoutState>((set) => ({
  sidebarCollapsed: false,
  toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
  speedLimit: 0,
  freeSpace: '-- GB',
  appVersion: '0.1.0',
}));
