import { useEffect } from 'react';
import { useSettingsStore } from '@/stores/settingsStore';

/**
 * Applies global DOM effects based on app config:
 * - dark class on :root (theme)
 * - compact-mode class on body
 * - --color-accent CSS variable on :root
 */
export function useAppEffects() {
  const config = useSettingsStore((s) => s.config);

  useEffect(() => {
    if (!config?.theme) return;

    const root = document.documentElement;
    const prefersDark =
      typeof window !== 'undefined' &&
      typeof window.matchMedia === 'function' &&
      window.matchMedia('(prefers-color-scheme: dark)').matches;
    const isDark = config.theme === 'dark' || (config.theme === 'auto' && prefersDark);
    root.classList.toggle('dark', isDark);

    try {
      localStorage.setItem('vortex-theme', config.theme);
    } catch {
      // SecurityError in private browsing — ignore
    }
  }, [config?.theme]);

  useEffect(() => {
    if (config === null) return;

    if (config.compactMode) {
      document.body.classList.add('compact-mode');
    } else {
      document.body.classList.remove('compact-mode');
    }

    return () => {
      document.body.classList.remove('compact-mode');
    };
  }, [config]);

  useEffect(() => {
    if (config === null) return;

    document.documentElement.style.setProperty('--color-accent', config.accentColor);

    return () => {
      document.documentElement.style.removeProperty('--color-accent');
    };
  }, [config]);
}
