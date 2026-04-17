import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, waitFor, act } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { createElement, type ReactNode } from 'react';

vi.mock('@/api/client', () => ({
  tauriInvoke: vi.fn(),
  queryClient: new QueryClient({ defaultOptions: { queries: { retry: false } } }),
}));

vi.mock('@/lib/toast', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
  },
}));

import { tauriInvoke } from '@/api/client';
import { useTauriQuery, useTauriMutation } from '@/api/hooks';
import { toast } from '@/lib/toast';

function makeWrapper() {
  const testClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: testClient }, children);
}

describe('useTauriQuery', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('should call tauriInvoke with command and args', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce({ id: '1' });
    const { result } = renderHook(
      () => useTauriQuery('get_download', { id: '1' }),
      { wrapper: makeWrapper() }
    );
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(tauriInvoke).toHaveBeenCalledWith('get_download', { id: '1' });
  });

  it('should return data on success', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce({ id: '1', fileName: 'test.zip' });
    const { result } = renderHook(
      () => useTauriQuery<{ id: string; fileName: string }>('get_download', { id: '1' }),
      { wrapper: makeWrapper() }
    );
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data).toEqual({ id: '1', fileName: 'test.zip' });
  });

  it('should expose error on failure', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('not found'));
    const { result } = renderHook(
      () => useTauriQuery('get_download', { id: '99' }),
      { wrapper: makeWrapper() }
    );
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.error?.message).toBe('not found');
  });

  it('should call tauriInvoke without args when none provided', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce([]);
    const { result } = renderHook(
      () => useTauriQuery('list_downloads'),
      { wrapper: makeWrapper() }
    );
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(tauriInvoke).toHaveBeenCalledWith('list_downloads', undefined);
  });
});

describe('useTauriMutation', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('should call tauriInvoke with command on mutate', async () => {
    vi.mocked(tauriInvoke).mockResolvedValueOnce(null);
    const { result } = renderHook(
      () => useTauriMutation('download_pause'),
      { wrapper: makeWrapper() }
    );
    await act(async () => {
      result.current.mutate({ id: '1' } as Record<string, unknown>);
    });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(tauriInvoke).toHaveBeenCalledWith('download_pause', { id: '1' });
  });

  it('should expose error when mutation fails', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('pause failed'));
    const { result } = renderHook(
      () => useTauriMutation('download_pause'),
      { wrapper: makeWrapper() }
    );
    await act(async () => {
      result.current.mutate({ id: '1' } as Record<string, unknown>);
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.error?.message).toBe('pause failed');
  });

  it('should surface toast.error by default when mutation fails and no onError is provided', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('boom'));
    const { result } = renderHook(
      () => useTauriMutation('download_pause'),
      { wrapper: makeWrapper() }
    );
    await act(async () => {
      result.current.mutate({ id: '1' } as Record<string, unknown>);
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(toast.error).toHaveBeenCalledWith('boom');
  });

  it('should NOT surface toast.error when silentError is true', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('quiet'));
    const { result } = renderHook(
      () => useTauriMutation('download_pause', { silentError: true }),
      { wrapper: makeWrapper() }
    );
    await act(async () => {
      result.current.mutate({ id: '1' } as Record<string, unknown>);
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(toast.error).not.toHaveBeenCalled();
  });

  it('should NOT surface toast.error when a custom onError is provided', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('custom'));
    const customOnError = vi.fn();
    const { result } = renderHook(
      () => useTauriMutation('download_pause', { onError: customOnError }),
      { wrapper: makeWrapper() }
    );
    await act(async () => {
      result.current.mutate({ id: '1' } as Record<string, unknown>);
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(customOnError).toHaveBeenCalled();
    expect(toast.error).not.toHaveBeenCalled();
  });

  it('should use errorMessage mapper when provided', async () => {
    vi.mocked(tauriInvoke).mockRejectedValueOnce(new Error('raw'));
    const { result } = renderHook(
      () =>
        useTauriMutation('download_pause', {
          errorMessage: (err) => `Mapped: ${err.message}`,
        }),
      { wrapper: makeWrapper() }
    );
    await act(async () => {
      result.current.mutate({ id: '1' } as Record<string, unknown>);
    });
    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(toast.error).toHaveBeenCalledWith('Mapped: raw');
  });
});
