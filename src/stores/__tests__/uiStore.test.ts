import { describe, it, expect, beforeEach } from 'vitest';
import { useUiStore } from '@/stores/uiStore';

beforeEach(() => {
  useUiStore.setState({
    selectedDownloadId: null,
    detailsPanelOpen: false,
    filterBarExpanded: false,
  });
});

describe('useUiStore — selectDownload', () => {
  it('should set selectedDownloadId', () => {
    useUiStore.getState().selectDownload('42');
    expect(useUiStore.getState().selectedDownloadId).toBe('42');
  });

  it('should clear selectedDownloadId when passed null', () => {
    useUiStore.getState().selectDownload('42');
    useUiStore.getState().selectDownload(null);
    expect(useUiStore.getState().selectedDownloadId).toBeNull();
  });
});

describe('useUiStore — setDetailsPanelOpen', () => {
  it('should open details panel', () => {
    useUiStore.getState().setDetailsPanelOpen(true);
    expect(useUiStore.getState().detailsPanelOpen).toBe(true);
  });

  it('should close details panel', () => {
    useUiStore.getState().setDetailsPanelOpen(true);
    useUiStore.getState().setDetailsPanelOpen(false);
    expect(useUiStore.getState().detailsPanelOpen).toBe(false);
  });
});

describe('useUiStore — toggleFilterBar', () => {
  it('should toggle filterBarExpanded from false to true', () => {
    useUiStore.getState().toggleFilterBar();
    expect(useUiStore.getState().filterBarExpanded).toBe(true);
  });

  it('should toggle filterBarExpanded from true to false', () => {
    useUiStore.setState({ filterBarExpanded: true, selectedDownloadId: null, detailsPanelOpen: false });
    useUiStore.getState().toggleFilterBar();
    expect(useUiStore.getState().filterBarExpanded).toBe(false);
  });
});
