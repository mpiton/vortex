import { createContext, useEffect, useState } from 'react';
import type { Theme } from '@/types/layout';

interface ThemeContextValue {
  theme: Theme;
  setTheme: (t: Theme) => void;
  resolvedTheme: 'light' | 'dark';
}

export const ThemeContext = createContext<ThemeContextValue | null>(null);

interface ThemeProviderProps {
  children: React.ReactNode;
  defaultTheme?: Theme;
}

function readStoredTheme(fallback: Theme): Theme {
  try {
    const stored = localStorage.getItem('vortex-theme');
    if (stored === 'light' || stored === 'dark' || stored === 'auto') {
      return stored;
    }
  } catch {
    // SecurityError in private browsing — fall through
  }
  return fallback;
}

function getSystemDark(): boolean {
  return typeof window.matchMedia === 'function'
    ? window.matchMedia('(prefers-color-scheme: dark)').matches
    : false;
}

export function ThemeProvider({ children, defaultTheme = 'auto' }: ThemeProviderProps) {
  const [theme, setThemeState] = useState<Theme>(() => readStoredTheme(defaultTheme));

  const [systemDark, setSystemDark] = useState(getSystemDark);

  useEffect(() => {
    if (typeof window.matchMedia !== 'function') return;
    const mq = window.matchMedia('(prefers-color-scheme: dark)');
    const handler = (e: MediaQueryListEvent) => setSystemDark(e.matches);
    mq.addEventListener('change', handler);
    return () => mq.removeEventListener('change', handler);
  }, []);

  const resolvedTheme: 'light' | 'dark' =
    theme === 'auto' ? (systemDark ? 'dark' : 'light') : theme;

  useEffect(() => {
    try {
      localStorage.setItem('vortex-theme', theme);
    } catch {
      // QuotaExceededError or SecurityError — ignore
    }
    const root = document.documentElement;
    if (resolvedTheme === 'dark') {
      root.classList.add('dark');
    } else {
      root.classList.remove('dark');
    }
  }, [theme, resolvedTheme]);

  const setTheme = (t: Theme) => setThemeState(t);

  return (
    <ThemeContext.Provider value={{ theme, setTheme, resolvedTheme }}>
      {children}
    </ThemeContext.Provider>
  );
}

