import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { invoke } from '@tauri-apps/api/core';
import type { ReactNode } from 'react';
import { usePackageByExternalId } from '../usePackageByExternalId';

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

describe('usePackageByExternalId', () => {
  it('calls invoke with externalId arg when defined', async () => {
    mockInvoke.mockResolvedValue({ packageId: 'p1', packageName: 'Mix' });

    const { result } = renderHook(
      () => usePackageByExternalId('youtube:playlist:abc'),
      { wrapper: wrapper() },
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(mockInvoke).toHaveBeenCalledWith(
      'package_find_by_external_id',
      { externalId: 'youtube:playlist:abc' },
    );
  });

  it('returns the PackageSummary from the IPC response', async () => {
    mockInvoke.mockResolvedValue({ packageId: 'p1', packageName: 'Mix' });

    const { result } = renderHook(
      () => usePackageByExternalId('youtube:playlist:abc'),
      { wrapper: wrapper() },
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(result.current.data).toEqual({ packageId: 'p1', packageName: 'Mix' });
  });

  it('does not call invoke when externalId is undefined', async () => {
    const { result } = renderHook(
      () => usePackageByExternalId(undefined),
      { wrapper: wrapper() },
    );

    // Give TanStack Query a tick to potentially fire — it should not.
    await new Promise((r) => setTimeout(r, 50));

    expect(mockInvoke).not.toHaveBeenCalled();
    expect(result.current.data).toBeUndefined();
  });

  it('returns null when no package matches', async () => {
    mockInvoke.mockResolvedValue(null);

    const { result } = renderHook(
      () => usePackageByExternalId('youtube:playlist:xyz'),
      { wrapper: wrapper() },
    );
    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(result.current.data).toBeNull();
  });
});
