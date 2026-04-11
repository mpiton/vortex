import { create } from 'zustand';
import { tauriInvoke } from '@/api/client';
import type { AppConfig, AppConfigPatch } from '@/types/settings';

interface SettingsStoreState {
  config: AppConfig | null;
  isLoading: boolean;
  error: string | null;
  setConfig: (config: AppConfig) => void;
  updateConfig: (partial: AppConfigPatch) => Promise<void>;
  clearError: () => void;
}

// Note: 'settings_update' IPC command will be registered in task 23.
// updateConfig applies optimistically, then attempts IPC; failure is logged, not thrown.
export const useSettingsStore = create<SettingsStoreState>((set) => ({
  config: null,
  isLoading: false,
  error: null,
  setConfig: (config) => set({ config, error: null }),
  clearError: () => set({ error: null }),
  updateConfig: async (partial) => {
    set({ isLoading: true, error: null });
    set((s) => ({
      config: s.config ? { ...s.config, ...partial } : s.config,
    }));
    try {
      await tauriInvoke('settings_update', partial);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      set({ error: message });
    } finally {
      set({ isLoading: false });
    }
  },
}));
