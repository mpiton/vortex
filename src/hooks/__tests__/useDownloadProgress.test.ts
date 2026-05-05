import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook } from "@testing-library/react";
import { useDownloadProgress } from "@/hooks/useDownloadProgress";

vi.mock("@/hooks/useTauriEvent", () => ({
  useTauriEvent: vi.fn(),
}));

vi.mock("@/stores/downloadStore", () => ({
  useDownloadStore: Object.assign(vi.fn(), {
    getState: vi.fn(),
  }),
}));

import { useTauriEvent } from "@/hooks/useTauriEvent";
import { useDownloadStore } from "@/stores/downloadStore";

describe("useDownloadProgress", () => {
  const mockUpdateProgress = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(useDownloadStore.getState).mockReturnValue({
      updateProgress: mockUpdateProgress,
      progressMap: {},
      countByState: {},
      waitMap: {},
      removeProgress: vi.fn(),
      updateCountByState: vi.fn(),
      clearAllProgress: vi.fn(),
      setWait: vi.fn(),
      clearWait: vi.fn(),
    });
  });

  it("should subscribe to download-progress event", () => {
    renderHook(() => useDownloadProgress());
    expect(useTauriEvent).toHaveBeenCalledWith("download-progress", expect.any(Function));
  });

  it("should call updateProgress with string id and bytes when event fires", () => {
    vi.mocked(useTauriEvent).mockImplementationOnce((_event, callback) => {
      callback({ id: 42, downloadedBytes: 500, totalBytes: 1000 });
    });
    renderHook(() => useDownloadProgress());
    expect(mockUpdateProgress).toHaveBeenCalledWith("42", 500, 1000);
  });

  it("should use getState() to avoid unnecessary re-renders", () => {
    renderHook(() => useDownloadProgress());
    const [, callback] = vi.mocked(useTauriEvent).mock.calls[0] as [string, (p: unknown) => void];
    callback({ id: 1, downloadedBytes: 100, totalBytes: 200 });
    expect(useDownloadStore.getState).toHaveBeenCalled();
  });
});
