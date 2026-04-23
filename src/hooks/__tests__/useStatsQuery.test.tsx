import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import type { ReactNode } from 'react';
import { useStatsQuery } from '../useStatsQuery';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

const mockInvoke = vi.mocked(invoke);

function wrapper() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, staleTime: 0 } },
  });
  return function Wrapper({ children }: { children: ReactNode }) {
    return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
  };
}

beforeEach(() => {
  mockInvoke.mockReset();
});

describe('useStatsQuery', () => {
  it('invokes stats_get with the requested period', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'stats_get') {
        return {
          totalDownloadedBytes: 1_000,
          totalFiles: 2,
          avgSpeed: 500,
          peakSpeed: 1_000,
          successRate: 1,
          dailyVolumes: [],
          topHosts: [],
        };
      }
      if (cmd === 'stats_top_modules') return [];
      if (cmd === 'history_list') return [];
      return null;
    });

    const { result } = renderHook(() => useStatsQuery('7d'), { wrapper: wrapper() });
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(mockInvoke).toHaveBeenCalledWith('stats_get', { period: '7d' });
    expect(result.current.stats?.totalFiles).toBe(2);
  });

  it('passes dateFrom cutoff to history_list for bounded periods', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'stats_get') {
        return {
          totalDownloadedBytes: 0,
          totalFiles: 0,
          avgSpeed: 0,
          peakSpeed: 0,
          successRate: 1,
          dailyVolumes: [],
          topHosts: [],
        };
      }
      if (cmd === 'stats_top_modules') return [];
      if (cmd === 'history_list') return [];
      return null;
    });

    const { result } = renderHook(() => useStatsQuery('7d'), { wrapper: wrapper() });
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    const historyCall = mockInvoke.mock.calls.find(([cmd]) => cmd === 'history_list');
    expect(historyCall).toBeDefined();
    const args = historyCall?.[1] as { dateFrom?: number; limit?: number };
    expect(args?.limit).toBe(500);
    expect(args?.dateFrom).toBeDefined();
    expect(typeof args?.dateFrom).toBe('number');
  });

  it('omits dateFrom for all-time period', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'stats_get') {
        return {
          totalDownloadedBytes: 0,
          totalFiles: 0,
          avgSpeed: 0,
          peakSpeed: 0,
          successRate: 1,
          dailyVolumes: [],
          topHosts: [],
        };
      }
      if (cmd === 'stats_top_modules') return [];
      if (cmd === 'history_list') return [];
      return null;
    });

    const { result } = renderHook(() => useStatsQuery('all'), { wrapper: wrapper() });
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    const historyCall = mockInvoke.mock.calls.find(([cmd]) => cmd === 'history_list');
    const args = historyCall?.[1] as { dateFrom?: number; limit?: number };
    expect(args?.limit).toBe(500);
    expect(args?.dateFrom).toBeUndefined();
  });

  it('refetches all queries when period changes', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'stats_get') {
        return {
          totalDownloadedBytes: 0,
          totalFiles: 0,
          avgSpeed: 0,
          peakSpeed: 0,
          successRate: 1,
          dailyVolumes: [],
          topHosts: [],
        };
      }
      if (cmd === 'stats_top_modules') return [];
      if (cmd === 'history_list') return [];
      return null;
    });

    const { result, rerender } = renderHook(({ p }: { p: '7d' | '30d' | 'all' }) => useStatsQuery(p), {
      wrapper: wrapper(),
      initialProps: { p: '7d' as '7d' | '30d' | 'all' },
    });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    rerender({ p: '30d' });
    await waitFor(() =>
      expect(
        mockInvoke.mock.calls.some(([cmd, args]) => cmd === 'stats_get' && (args as { period: string }).period === '30d'),
      ).toBe(true),
    );
  });

  it('limits top modules to 5', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'stats_get') {
        return {
          totalDownloadedBytes: 0,
          totalFiles: 0,
          avgSpeed: 0,
          peakSpeed: 0,
          successRate: 1,
          dailyVolumes: [],
          topHosts: [],
        };
      }
      if (cmd === 'stats_top_modules') return [];
      if (cmd === 'history_list') return [];
      return null;
    });
    const { result } = renderHook(() => useStatsQuery('all'), { wrapper: wrapper() });
    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(mockInvoke).toHaveBeenCalledWith('stats_top_modules', { limit: 5 });
  });

  it('surfaces query error', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'stats_get') throw new Error('backend boom');
      if (cmd === 'stats_top_modules') return [];
      if (cmd === 'history_list') return [];
      return null;
    });
    const { result } = renderHook(() => useStatsQuery('7d'), { wrapper: wrapper() });
    await waitFor(() => expect(result.current.error).not.toBeNull());
    expect(result.current.error?.message).toBe('backend boom');
  });
});
