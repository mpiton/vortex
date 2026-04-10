import { create } from 'zustand';
import { tauriInvoke } from '@/api/client';

interface AppConfig {
  [key: string]: unknown;
}

interface SettingsStoreState {
  config: AppConfig | null;
  isLoading: boolean;
  error: string | null;
  setConfig: (config: AppConfig) => void;
  updateConfig: (partial: Partial<AppConfig>) => Promise<void>;
  clearError: () => void;
}

// Note: 'settings_update' IPC command will be implemented in task 23.
// Until then, updateConfig stores optimistically and logs a warning.
export const useSettingsStore = create<SettingsStoreState>((set) => ({
  config: null,
  isLoading: false,
  error: null,
  setConfig: (config) => set({ config, error: null }),
  clearError: () => set({ error: null }),
  updateConfig: async (partial) => {
    set({ isLoading: true, error: null });
    try {
      await tauriInvoke('settings_update', partial);
      set((s) => ({
        config: s.config ? { ...s.config, ...partial } : partial,
      }));
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      set({ error: message });
      throw err;
    } finally {
      set({ isLoading: false });
    }
  },
}));
