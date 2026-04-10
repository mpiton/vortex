import { create } from 'zustand';

interface UiStoreState {
  selectedDownloadId: string | null;
  selectedDownloadIds: string[];
  detailsPanelOpen: boolean;
  filterBarExpanded: boolean;
  selectDownload: (id: string | null) => void;
  setSelectedDownloadIds: (ids: string[]) => void;
  toggleDownloadSelection: (id: string) => void;
  clearSelection: () => void;
  setDetailsPanelOpen: (open: boolean) => void;
  toggleFilterBar: () => void;
}

export const useUiStore = create<UiStoreState>((set) => ({
  selectedDownloadId: null,
  selectedDownloadIds: [],
  detailsPanelOpen: false,
  filterBarExpanded: false,
  selectDownload: (id) => set({ selectedDownloadId: id }),
  setSelectedDownloadIds: (ids) => set({ selectedDownloadIds: ids }),
  toggleDownloadSelection: (id) =>
    set((s) => {
      const removing = s.selectedDownloadIds.includes(id);
      const nextIds = removing
        ? s.selectedDownloadIds.filter((i) => i !== id)
        : [...s.selectedDownloadIds, id];
      return {
        selectedDownloadIds: nextIds,
        selectedDownloadId: removing && s.selectedDownloadId === id
          ? null
          : s.selectedDownloadId,
      };
    }),
  clearSelection: () => set({ selectedDownloadIds: [], selectedDownloadId: null }),
  setDetailsPanelOpen: (open) => set({ detailsPanelOpen: open }),
  toggleFilterBar: () => set((s) => ({ filterBarExpanded: !s.filterBarExpanded })),
}));
