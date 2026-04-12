import { useEffect } from 'react';
import { useSettingsStore } from '@/stores/settingsStore';

/**
 * Applies global DOM effects based on app config:
 * - compact-mode class on body
 * - --color-accent CSS variable on :root
 */
export function useAppEffects() {
  const config = useSettingsStore((s) => s.config);

  useEffect(() => {
    if (config === null) return;

    if (config.compactMode) {
      document.body.classList.add('compact-mode');
    } else {
      document.body.classList.remove('compact-mode');
    }
  }, [config]);

  useEffect(() => {
    if (config === null) return;

    document.documentElement.style.setProperty('--color-accent', config.accentColor);
  }, [config]);
}
