import { create } from "zustand";

interface DownloadProgress {
  id: string;
  downloadedBytes: number;
  totalBytes: number;
  speedBytesPerSec: number;
  lastSampleBytes: number;
  lastSampleTime: number;
}

interface DownloadStoreState {
  progressMap: Record<string, DownloadProgress>;
  countByState: Record<string, number>;

  updateProgress: (id: string, downloadedBytes: number, totalBytes: number) => void;
  removeProgress: (id: string) => void;
  updateCountByState: (counts: Record<string, number>) => void;
  clearAllProgress: () => void;
}

export const selectTotalSpeed = (state: DownloadStoreState): number =>
  Object.values(state.progressMap).reduce((sum, p) => sum + p.speedBytesPerSec, 0);

export const selectActiveCount = (state: DownloadStoreState): number =>
  state.countByState["Downloading"] ?? 0;

export const useDownloadStore = create<DownloadStoreState>((set) => ({
  progressMap: {},
  countByState: {},

  updateProgress: (id, downloadedBytes, totalBytes) =>
    set((s) => {
      const prev = s.progressMap[id];
      const now = Date.now();
      let speedBytesPerSec = prev?.speedBytesPerSec ?? 0;

      if (prev && now > prev.lastSampleTime) {
        const deltaBytes = downloadedBytes - prev.lastSampleBytes;
        const deltaSec = (now - prev.lastSampleTime) / 1000;
        if (deltaSec > 0) {
          speedBytesPerSec = Math.max(0, deltaBytes / deltaSec);
        }
      }

      return {
        progressMap: {
          ...s.progressMap,
          [id]: {
            id,
            downloadedBytes,
            totalBytes,
            speedBytesPerSec,
            lastSampleBytes: downloadedBytes,
            lastSampleTime: now,
          },
        },
      };
    }),

  removeProgress: (id) =>
    set((s) => {
      const { [id]: _removed, ...rest } = s.progressMap;
      return { progressMap: rest };
    }),

  updateCountByState: (counts) => set({ countByState: counts }),

  clearAllProgress: () => set({ progressMap: {} }),
}));
