import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook } from '@testing-library/react';
import { useTauriEvent } from './useTauriEvent';

// Mock @tauri-apps/api/event
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
}));

import { listen } from '@tauri-apps/api/event';

describe('useTauriEvent', () => {
  let mockUnlisten: ReturnType<typeof vi.fn>;
  let capturedCallback: ((event: { payload: unknown }) => void) | null;

  beforeEach(() => {
    vi.clearAllMocks();
    mockUnlisten = vi.fn();
    capturedCallback = null;
    vi.mocked(listen).mockImplementation((_event, callback) => {
      capturedCallback = callback as (event: { payload: unknown }) => void;
      return Promise.resolve(mockUnlisten);
    });
  });

  it('should subscribe to the specified event on mount', () => {
    const callback = vi.fn();
    renderHook(() => useTauriEvent('download-progress', callback));
    expect(listen).toHaveBeenCalledWith('download-progress', expect.any(Function));
  });

  it('should call callback with payload when event fires', () => {
    const callback = vi.fn();
    renderHook(() => useTauriEvent('download-started', callback));
    capturedCallback?.({ payload: { id: 42 } });
    expect(callback).toHaveBeenCalledWith({ id: 42 });
  });

  it('should cleanup listener on unmount', async () => {
    const callback = vi.fn();
    const { unmount } = renderHook(() => useTauriEvent('download-progress', callback));
    unmount();
    await vi.waitFor(() => {
      expect(mockUnlisten).toHaveBeenCalled();
    });
  });

  it('should re-subscribe when eventName changes and cleanup old listener', async () => {
    const callback = vi.fn();
    const { rerender } = renderHook(
      ({ eventName }) => useTauriEvent(eventName, callback),
      { initialProps: { eventName: 'download-started' } }
    );
    expect(listen).toHaveBeenCalledTimes(1);
    rerender({ eventName: 'download-completed' });
    expect(listen).toHaveBeenCalledTimes(2);
    expect(listen).toHaveBeenLastCalledWith('download-completed', expect.any(Function));
    await vi.waitFor(() => {
      expect(mockUnlisten).toHaveBeenCalled();
    });
  });

  it('should not call callback after cleanup (cancelled flag)', async () => {
    const callback = vi.fn();
    const { unmount } = renderHook(() => useTauriEvent('test-event', callback));
    unmount();
    // Simulate event arriving after unmount
    capturedCallback?.({ payload: 'late-data' });
    expect(callback).not.toHaveBeenCalled();
  });

  it('should handle listen rejection without unhandled promise', async () => {
    vi.mocked(listen).mockRejectedValueOnce(new Error('backend unavailable'));
    const callback = vi.fn();
    const { unmount } = renderHook(() => useTauriEvent('test-event', callback));
    // Should not throw unhandled rejection
    unmount();
    await vi.waitFor(() => {
      expect(listen).toHaveBeenCalled();
    });
  });

  it('should use latest callback without re-subscribing', () => {
    const callback1 = vi.fn();
    const callback2 = vi.fn();
    const { rerender } = renderHook(
      ({ cb }) => useTauriEvent('test-event', cb),
      { initialProps: { cb: callback1 } }
    );
    rerender({ cb: callback2 });
    expect(listen).toHaveBeenCalledTimes(1);
    capturedCallback?.({ payload: 'data' });
    expect(callback2).toHaveBeenCalledWith('data');
    expect(callback1).not.toHaveBeenCalled();
  });
});
