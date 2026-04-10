import { describe, it, expect, vi, beforeEach } from 'vitest';
import { useSettingsStore } from '@/stores/settingsStore';

vi.mock('@/api/client', () => ({
  tauriInvoke: vi.fn(),
  queryClient: {},
}));

import { tauriInvoke } from '@/api/client';

beforeEach(() => {
  vi.clearAllMocks();
  useSettingsStore.setState({ config: null, isLoading: false, error: null });
});

describe('useSettingsStore — setConfig', () => {
  it('should set config', () => {
    useSettingsStore.getState().setConfig({ theme: 'dark' });
    expect(useSettingsStore.getState().config).toEqual({ theme: 'dark' });
  });
});

describe('useSettingsStore — updateConfig', () => {
  it('should call tauriInvoke with settings command', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(null);
    useSettingsStore.setState({ config: { theme: 'light' }, isLoading: false });
    await useSettingsStore.getState().updateConfig({ theme: 'dark' });
    expect(tauriInvoke).toHaveBeenCalledWith('settings_update', { theme: 'dark' });
  });

  it('should merge partial config on success', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(null);
    useSettingsStore.setState({ config: { theme: 'light', lang: 'fr' }, isLoading: false });
    await useSettingsStore.getState().updateConfig({ theme: 'dark' });
    expect(useSettingsStore.getState().config).toEqual({ theme: 'dark', lang: 'fr' });
  });

  it('should set config with partial when config was null', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(null);
    await useSettingsStore.getState().updateConfig({ theme: 'dark' });
    expect(useSettingsStore.getState().config).toEqual({ theme: 'dark' });
  });

  it('should reset isLoading to false after success', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(null);
    await useSettingsStore.getState().updateConfig({ theme: 'dark' });
    expect(useSettingsStore.getState().isLoading).toBe(false);
  });

  it('should set error and re-throw on failure', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('server error'));
    await expect(useSettingsStore.getState().updateConfig({ theme: 'dark' })).rejects.toThrow('server error');
    expect(useSettingsStore.getState().isLoading).toBe(false);
    expect(useSettingsStore.getState().error).toBe('server error');
  });

  it('should clear error on next successful update', async () => {
    useSettingsStore.setState({ error: 'previous error' });
    vi.mocked(tauriInvoke).mockResolvedValueOnce(null);
    await useSettingsStore.getState().updateConfig({ theme: 'dark' });
    expect(useSettingsStore.getState().error).toBeNull();
  });
});
