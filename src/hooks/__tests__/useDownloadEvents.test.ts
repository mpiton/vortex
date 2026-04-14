import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook } from '@testing-library/react';
import { useDownloadEvents } from '@/hooks/useDownloadEvents';

vi.mock('@/hooks/useTauriEvent', () => ({
  useTauriEvent: vi.fn(),
}));

vi.mock('@/api/client', () => ({
  queryClient: {
    invalidateQueries: vi.fn(),
  },
  tauriInvoke: vi.fn(),
}));

vi.mock('@/api/queries', () => ({
  downloadQueries: {
    all: () => ['downloads'],
    lists: () => ['downloads', 'list'],
    countByState: () => ['downloads', 'countByState'],
  },
}));

import { useTauriEvent } from '@/hooks/useTauriEvent';
import { queryClient } from '@/api/client';

describe('useDownloadEvents', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('should subscribe to all download lifecycle events', () => {
    renderHook(() => useDownloadEvents());
    const subscribedEvents = vi.mocked(useTauriEvent).mock.calls.map(([event]) => event);
    expect(subscribedEvents).toContain('download-created');
    expect(subscribedEvents).toContain('download-started');
    expect(subscribedEvents).toContain('download-paused');
    expect(subscribedEvents).toContain('download-resumed');
    expect(subscribedEvents).toContain('download-resumed-from-wait');
    expect(subscribedEvents).toContain('download-completed');
    expect(subscribedEvents).toContain('download-failed');
    expect(subscribedEvents).toContain('download-retrying');
    expect(subscribedEvents).toContain('download-cancelled');
    expect(subscribedEvents).toContain('download-waiting');
    expect(subscribedEvents).toContain('download-checking');
    expect(subscribedEvents).toContain('download-removed');
    expect(subscribedEvents).toContain('download-extracting');
  });

  it('should invalidate download list queries on download-created', () => {
    vi.mocked(useTauriEvent).mockImplementation((event, callback) => {
      if (event === 'download-created') callback({ id: 1 });
    });
    renderHook(() => useDownloadEvents());
    expect(queryClient.invalidateQueries).toHaveBeenCalledWith({
      queryKey: ['downloads'],
    });
  });

  it('should invalidate download list queries on download-completed', () => {
    vi.mocked(useTauriEvent).mockImplementation((event, callback) => {
      if (event === 'download-completed') callback({ id: 2 });
    });
    renderHook(() => useDownloadEvents());
    expect(queryClient.invalidateQueries).toHaveBeenCalledWith({
      queryKey: ['downloads'],
    });
  });

  it('should invalidate download list queries on download-failed', () => {
    vi.mocked(useTauriEvent).mockImplementation((event, callback) => {
      if (event === 'download-failed') callback({ id: 3, error: 'timeout' });
    });
    renderHook(() => useDownloadEvents());
    expect(queryClient.invalidateQueries).toHaveBeenCalledWith({
      queryKey: ['downloads'],
    });
  });

  it('should invalidate download list queries on download-cancelled', () => {
    vi.mocked(useTauriEvent).mockImplementation((event, callback) => {
      if (event === 'download-cancelled') callback({ id: 4 });
    });
    renderHook(() => useDownloadEvents());
    expect(queryClient.invalidateQueries).toHaveBeenCalledWith({
      queryKey: ['downloads'],
    });
  });

  it('should invalidate download list queries on download-retrying', () => {
    vi.mocked(useTauriEvent).mockImplementation((event, callback) => {
      if (event === 'download-retrying') callback({ id: 5, attempt: 1 });
    });
    renderHook(() => useDownloadEvents());
    expect(queryClient.invalidateQueries).toHaveBeenCalledWith({
      queryKey: ['downloads'],
    });
  });

  it('should subscribe to exactly 13 lifecycle events', () => {
    renderHook(() => useDownloadEvents());
    expect(useTauriEvent).toHaveBeenCalledTimes(13);
  });
});
