import { create } from 'zustand';

interface DownloadProgress {
  id: string;
  downloadedBytes: number;
  totalBytes: number;
}

interface DownloadStoreState {
  progressMap: Record<string, DownloadProgress>;
  countByState: Record<string, number>;

  updateProgress: (id: string, downloadedBytes: number, totalBytes: number) => void;
  removeProgress: (id: string) => void;
  updateCountByState: (counts: Record<string, number>) => void;
  clearAllProgress: () => void;
}

export const selectTotalSpeed = (_state: DownloadStoreState): number => 0;

export const selectActiveCount = (state: DownloadStoreState): number =>
  state.countByState['Downloading'] ?? 0;

export const useDownloadStore = create<DownloadStoreState>((set) => ({
  progressMap: {},
  countByState: {},

  updateProgress: (id, downloadedBytes, totalBytes) =>
    set((s) => ({
      progressMap: {
        ...s.progressMap,
        [id]: { id, downloadedBytes, totalBytes },
      },
    })),

  removeProgress: (id) =>
    set((s) => {
      const { [id]: _removed, ...rest } = s.progressMap;
      return { progressMap: rest };
    }),

  updateCountByState: (counts) => set({ countByState: counts }),

  clearAllProgress: () => set({ progressMap: {} }),
}));
