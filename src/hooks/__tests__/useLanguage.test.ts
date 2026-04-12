import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';

vi.mock('@/stores/settingsStore', () => ({
  useSettingsStore: vi.fn(),
}));

import { useSettingsStore } from '@/stores/settingsStore';
import { useLanguage } from '@/hooks/useLanguage';

describe('useLanguage', () => {
  const mockUpdateConfig = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(useSettingsStore).mockReturnValue(mockUpdateConfig);
  });

  it('should return current language from i18n', () => {
    const { result } = renderHook(() => useLanguage());
    expect(result.current.current).toBe('en');
  });

  it('should return available languages including en and fr', () => {
    const { result } = renderHook(() => useLanguage());
    expect(result.current.availableLanguages).toContain('en');
    expect(result.current.availableLanguages).toContain('fr');
  });

  it('should call updateConfig with locale fr when setLanguage called with fr', async () => {
    const { result } = renderHook(() => useLanguage());

    await act(async () => {
      result.current.setLanguage('fr');
    });

    expect(mockUpdateConfig).toHaveBeenCalledWith({ locale: 'fr' });
  });

  it('should call updateConfig with locale en when setLanguage called with en', async () => {
    const { result } = renderHook(() => useLanguage());

    await act(async () => {
      result.current.setLanguage('en');
    });

    expect(mockUpdateConfig).toHaveBeenCalledWith({ locale: 'en' });
  });
});
