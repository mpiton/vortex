import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook } from '@testing-library/react';

vi.mock('@/stores/settingsStore', () => ({
  useSettingsStore: vi.fn(),
}));

import { useSettingsStore } from '@/stores/settingsStore';
import { useAppEffects } from '@/hooks/useAppEffects';
import type { AppConfig } from '@/types/settings';

const baseConfig: AppConfig = {
  downloadDir: null,
  maxConcurrentDownloads: 3,
  maxSegmentsPerDownload: 8,
  speedLimitBytesPerSec: null,
  autoExtract: false,
  theme: 'light',
  locale: 'en',
  clipboardMonitoring: false,
  startMinimized: false,
  notificationsEnabled: false,
  soundEnabled: false,
  confirmDelete: false,
  subfolderPerPackage: false,
  maxRetries: 3,
  retryDelaySeconds: 5,
  verifyChecksums: false,
  preAllocateSpace: false,
  proxyType: 'none',
  proxyUrl: null,
  userAgent: 'Vortex/1.0',
  dnsOverHttps: false,
  connectionTimeoutSeconds: 30,
  webInterfaceEnabled: false,
  webInterfacePort: 9666,
  restApiEnabled: false,
  apiKey: '',
  websocketEnabled: false,
  minFileSizeMb: 1,
  excludedDomains: [],
  excludedExtensions: [],
  accentColor: '#4F46E5',
  compactMode: false,
};

describe('useAppEffects', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    document.body.classList.remove('compact-mode');
    document.documentElement.style.removeProperty('--color-accent');
  });

  afterEach(() => {
    document.body.classList.remove('compact-mode');
    document.documentElement.style.removeProperty('--color-accent');
  });

  it('should add compact-mode class when compactMode is true', () => {
    vi.mocked(useSettingsStore).mockReturnValue({ ...baseConfig, compactMode: true });

    renderHook(() => useAppEffects());

    expect(document.body.classList.contains('compact-mode')).toBe(true);
  });

  it('should not add compact-mode class when compactMode is false', () => {
    vi.mocked(useSettingsStore).mockReturnValue({ ...baseConfig, compactMode: false });

    renderHook(() => useAppEffects());

    expect(document.body.classList.contains('compact-mode')).toBe(false);
  });

  it('should remove compact-mode class when compactMode changes to false', () => {
    document.body.classList.add('compact-mode');
    vi.mocked(useSettingsStore).mockReturnValue({ ...baseConfig, compactMode: false });

    renderHook(() => useAppEffects());

    expect(document.body.classList.contains('compact-mode')).toBe(false);
  });

  it('should set --color-accent CSS variable with accentColor', () => {
    vi.mocked(useSettingsStore).mockReturnValue({ ...baseConfig, accentColor: '#A855F7' });

    renderHook(() => useAppEffects());

    expect(document.documentElement.style.getPropertyValue('--color-accent')).toBe('#A855F7');
  });

  it('should not apply effects when config is null', () => {
    vi.mocked(useSettingsStore).mockReturnValue(null);

    renderHook(() => useAppEffects());

    expect(document.body.classList.contains('compact-mode')).toBe(false);
    expect(document.documentElement.style.getPropertyValue('--color-accent')).toBe('');
  });
});
