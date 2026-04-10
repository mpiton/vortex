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
    set((s) => ({
      selectedDownloadIds: s.selectedDownloadIds.includes(id)
        ? s.selectedDownloadIds.filter((i) => i !== id)
        : [...s.selectedDownloadIds, id],
    })),
  clearSelection: () => set({ selectedDownloadIds: [] }),
  setDetailsPanelOpen: (open) => set({ detailsPanelOpen: open }),
  toggleFilterBar: () => set((s) => ({ filterBarExpanded: !s.filterBarExpanded })),
}));
