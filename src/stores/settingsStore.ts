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
// Until then, updateConfig applies the change optimistically without a backend call.
export const useSettingsStore = create<SettingsStoreState>((set) => ({
  config: null,
  isLoading: false,
  error: null,
  setConfig: (config) => set({ config, error: null }),
  clearError: () => set({ error: null }),
  updateConfig: async (partial) => {
    set({ isLoading: true, error: null });
    set((s) => ({
      config: s.config ? { ...s.config, ...partial } : partial,
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
