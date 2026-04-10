import { create } from 'zustand';

interface UiStoreState {
  selectedDownloadId: string | null;
  detailsPanelOpen: boolean;
  filterBarExpanded: boolean;
  selectDownload: (id: string | null) => void;
  setDetailsPanelOpen: (open: boolean) => void;
  toggleFilterBar: () => void;
}

export const useUiStore = create<UiStoreState>((set) => ({
  selectedDownloadId: null,
  detailsPanelOpen: false,
  filterBarExpanded: false,
  selectDownload: (id) => set({ selectedDownloadId: id }),
  setDetailsPanelOpen: (open) => set({ detailsPanelOpen: open }),
  toggleFilterBar: () => set((s) => ({ filterBarExpanded: !s.filterBarExpanded })),
}));
