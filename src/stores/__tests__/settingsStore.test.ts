import { describe, it, expect, vi, beforeEach } from 'vitest';
import { useSettingsStore } from '@/stores/settingsStore';

vi.mock('@/api/client', () => ({
  tauriInvoke: vi.fn(),
  queryClient: {},
}));

import { tauriInvoke } from '@/api/client';

import type { AppConfig } from '@/types/settings';

const baseConfig: AppConfig = {
  downloadDir: '/tmp',
  maxConcurrentDownloads: 4,
  maxSegmentsPerDownload: 8,
  speedLimitBytesPerSec: null,
  autoExtract: true,
  theme: 'light',
  locale: 'en',
  clipboardMonitoring: true,
  startMinimized: false,
  notificationsEnabled: true,
  soundEnabled: false,
  confirmDelete: true,
  subfolderPerPackage: false,
  maxRetries: 5,
  retryDelaySeconds: 10,
  verifyChecksums: true,
  preAllocateSpace: false,
  dynamicSplitEnabled: true,
  dynamicSplitMinRemainingMb: 4,
  proxyType: 'none',
  proxyUrl: null,
  userAgent: 'Vortex/1.0',
  dnsOverHttps: false,
  connectionTimeoutSeconds: 30,
  webInterfaceEnabled: false,
  webInterfacePort: 9876,
  restApiEnabled: true,
  apiKey: '',
  websocketEnabled: true,
  minFileSizeMb: 1,
  excludedDomains: [],
  excludedExtensions: [],
  accentColor: '#4F46E5',
  compactMode: false,
  historyRetentionDays: 30,
};

beforeEach(() => {
  vi.clearAllMocks();
  useSettingsStore.setState({ config: null, isLoading: false, error: null });
});

describe('useSettingsStore — setConfig', () => {
  it('should set config', () => {
    useSettingsStore.getState().setConfig({ ...baseConfig, theme: 'dark' });
    expect(useSettingsStore.getState().config?.theme).toBe('dark');
  });
});

describe('useSettingsStore — updateConfig', () => {
  it('should call tauriInvoke with settings command', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(null);
    useSettingsStore.setState({ config: baseConfig, isLoading: false });
    await useSettingsStore.getState().updateConfig({ theme: 'dark' });
    expect(tauriInvoke).toHaveBeenCalledWith('settings_update', { patch: { theme: 'dark' } });
  });

  it('should merge partial config on success', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(null);
    useSettingsStore.setState({ config: baseConfig, isLoading: false });
    await useSettingsStore.getState().updateConfig({ theme: 'dark' });
    expect(useSettingsStore.getState().config?.theme).toBe('dark');
    expect(useSettingsStore.getState().config?.locale).toBe('en');
  });

  it('should not create config from partial when config is null', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(null);
    await useSettingsStore.getState().updateConfig({ theme: 'dark' });
    expect(useSettingsStore.getState().config).toBeNull();
  });

  it('should reset isLoading to false after success', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(null);
    useSettingsStore.setState({ config: baseConfig, isLoading: false });
    await useSettingsStore.getState().updateConfig({ theme: 'dark' });
    expect(useSettingsStore.getState().isLoading).toBe(false);
  });

  it('should apply config optimistically before IPC call', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('unavailable'));
    useSettingsStore.setState({ config: baseConfig, isLoading: false, error: null });
    await useSettingsStore.getState().updateConfig({ theme: 'dark' });
    expect(useSettingsStore.getState().config?.theme).toBe('dark');
  });

  it('should set error but not throw on IPC failure', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('server error'));
    useSettingsStore.setState({ config: baseConfig, isLoading: false, error: null });
    await useSettingsStore.getState().updateConfig({ theme: 'dark' });
    expect(useSettingsStore.getState().isLoading).toBe(false);
    expect(useSettingsStore.getState().error).toBe('server error');
  });

  it('should clear error on next successful update', async () => {
    useSettingsStore.setState({ config: baseConfig, error: 'previous error' });
    vi.mocked(tauriInvoke).mockResolvedValueOnce(null);
    await useSettingsStore.getState().updateConfig({ theme: 'dark' });
    expect(useSettingsStore.getState().error).toBeNull();
  });
});
