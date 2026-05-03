import { describe, it, expect, beforeEach } from "vitest";
import { useDownloadStore, selectActiveCount, selectTotalSpeed } from "@/stores/downloadStore";

beforeEach(() => {
  useDownloadStore.setState({ progressMap: {}, countByState: {} });
});

describe("useDownloadStore — updateProgress", () => {
  it("should add entry to progressMap", () => {
    useDownloadStore.getState().updateProgress("1", 500, 1000);
    const entry = useDownloadStore.getState().progressMap["1"];
    expect(entry.id).toBe("1");
    expect(entry.downloadedBytes).toBe(500);
    expect(entry.totalBytes).toBe(1000);
    expect(entry.speedBytesPerSec).toBe(0);
  });

  it("should update existing entry", () => {
    useDownloadStore.getState().updateProgress("1", 500, 1000);
    useDownloadStore.getState().updateProgress("1", 800, 1000);
    expect(useDownloadStore.getState().progressMap["1"].downloadedBytes).toBe(800);
  });

  it("should handle multiple entries independently", () => {
    useDownloadStore.getState().updateProgress("1", 100, 200);
    useDownloadStore.getState().updateProgress("2", 300, 400);
    expect(Object.keys(useDownloadStore.getState().progressMap)).toHaveLength(2);
  });
});

describe("useDownloadStore — removeProgress", () => {
  it("should remove entry from progressMap", () => {
    useDownloadStore.getState().updateProgress("1", 500, 1000);
    useDownloadStore.getState().removeProgress("1");
    expect(useDownloadStore.getState().progressMap["1"]).toBeUndefined();
  });

  it("should not affect other entries when removing", () => {
    useDownloadStore.getState().updateProgress("1", 100, 200);
    useDownloadStore.getState().updateProgress("2", 300, 400);
    useDownloadStore.getState().removeProgress("1");
    expect(useDownloadStore.getState().progressMap["2"]).toBeDefined();
  });
});

describe("useDownloadStore — updateCountByState", () => {
  it("should update countByState", () => {
    useDownloadStore.getState().updateCountByState({ Downloading: 3, Paused: 1 });
    expect(useDownloadStore.getState().countByState).toEqual({ Downloading: 3, Paused: 1 });
  });

  it("should replace previous counts", () => {
    useDownloadStore.getState().updateCountByState({ Downloading: 3 });
    useDownloadStore.getState().updateCountByState({ Paused: 2 });
    expect(useDownloadStore.getState().countByState).toEqual({ Paused: 2 });
  });
});

describe("useDownloadStore — clearAllProgress", () => {
  it("should clear progressMap", () => {
    useDownloadStore.getState().updateProgress("1", 100, 200);
    useDownloadStore.getState().clearAllProgress();
    expect(useDownloadStore.getState().progressMap).toEqual({});
  });
});

describe("selectActiveCount", () => {
  it("should return Downloading count", () => {
    useDownloadStore.setState({ countByState: { Downloading: 5 }, progressMap: {} });
    expect(selectActiveCount(useDownloadStore.getState())).toBe(5);
  });

  it("should return 0 when no Downloading entries", () => {
    expect(selectActiveCount(useDownloadStore.getState())).toBe(0);
  });
});

describe("selectTotalSpeed", () => {
  it("should return 0 when no progress entries", () => {
    expect(selectTotalSpeed(useDownloadStore.getState())).toBe(0);
  });

  it("should sum speed across progress entries", () => {
    useDownloadStore.setState({
      progressMap: {
        "1": {
          id: "1",
          downloadedBytes: 500,
          totalBytes: 1000,
          speedBytesPerSec: 100,
          lastSampleBytes: 500,
          lastSampleTime: Date.now(),
        },
        "2": {
          id: "2",
          downloadedBytes: 300,
          totalBytes: 600,
          speedBytesPerSec: 200,
          lastSampleBytes: 300,
          lastSampleTime: Date.now(),
        },
      },
      countByState: {},
    });
    expect(selectTotalSpeed(useDownloadStore.getState())).toBe(300);
  });
});
