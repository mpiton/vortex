import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useSpeedHistory } from "../useSpeedHistory";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(null),
}));

const { mockGetState } = vi.hoisted(() => ({
  mockGetState: vi.fn(),
}));

vi.mock("@/stores/downloadStore", () => ({
  useDownloadStore: Object.assign(
    (selector: (s: { progressMap: Record<string, unknown> }) => unknown) =>
      selector({ progressMap: {} }),
    { getState: mockGetState },
  ),
}));

describe("useSpeedHistory", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockGetState.mockReturnValue({ progressMap: {} });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it("should sample immediately on mount", () => {
    const { result } = renderHook(() => useSpeedHistory("dl-1"));
    expect(result.current.length).toBe(1);
    expect(result.current[0].speed).toBe(0);
  });

  it("should collect speed samples over time", () => {
    mockGetState.mockReturnValue({
      progressMap: {
        "dl-1": {
          id: "dl-1",
          downloadedBytes: 524288,
          totalBytes: 1048576,
          speedBytesPerSec: 1024000,
          lastSampleBytes: 524288,
          lastSampleTime: Date.now(),
        },
      },
    });

    const { result } = renderHook(() => useSpeedHistory("dl-1"));

    expect(result.current.length).toBe(1);
    expect(result.current[0].speed).toBe(1024000);

    act(() => {
      vi.advanceTimersByTime(2000);
    });

    expect(result.current.length).toBe(2);

    act(() => {
      vi.advanceTimersByTime(2000);
    });

    expect(result.current.length).toBe(3);
  });
});
