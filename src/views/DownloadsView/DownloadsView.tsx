import { useCallback, useEffect, useMemo, useState } from 'react';
import { useTauriMutation, useTauriQuery } from '@/api/hooks';
import { downloadQueries } from '@/api/queries';
import { useDownloadProgress } from '@/hooks/useDownloadProgress';
import { subscribeShortcutAction, SHORTCUT_ACTIONS } from '@/lib/keyboardShortcuts';
import { useUiStore } from '@/stores/uiStore';
import type { DownloadView } from '@/types/download';
import type { FilterType } from './types';
import { SearchBar } from './SearchBar';
import { FilterBar } from './FilterBar';
import { ActionsBar } from './ActionsBar';
import { DownloadsTable, filterDownloads } from './DownloadsTable';
import { DownloadDetailsPanel } from '../DownloadDetailsPanel';

const INVALIDATE_KEYS = [
  downloadQueries.lists(),
  downloadQueries.countByState(),
] as const;

export function DownloadsView() {
  const [filter, setFilter] = useState<FilterType>('all');
  const [searchQuery, setSearchQuery] = useState('');
  const selectedDownloadId = useUiStore((s) => s.selectedDownloadId);
  const selectedDownloadIds = useUiStore((s) => s.selectedDownloadIds);
  const selectDownload = useUiStore((s) => s.selectDownload);
  const setSelectedDownloadIds = useUiStore((s) => s.setSelectedDownloadIds);
  const clearSelection = useUiStore((s) => s.clearSelection);

  useDownloadProgress();

  const pauseMut = useTauriMutation<void, { id: string }>('download_pause', {
    invalidateKeys: INVALIDATE_KEYS,
  });
  const resumeMut = useTauriMutation<void, { id: string }>('download_resume', {
    invalidateKeys: INVALIDATE_KEYS,
  });
  const removeMut = useTauriMutation<void, { id: string; deleteFiles: boolean }>(
    'download_remove',
    { invalidateKeys: INVALIDATE_KEYS },
  );

  const { data: downloads, isLoading } = useTauriQuery<DownloadView[]>(
    'download_list',
    undefined,
    { queryKey: downloadQueries.lists(), staleTime: 1000 },
  );

  const { data: countByState } = useTauriQuery<Record<string, number>>(
    'download_count_by_state',
    undefined,
    { queryKey: downloadQueries.countByState(), staleTime: 2000 },
  );

  const filteredDownloads = useMemo(
    () =>
      filterDownloads(downloads ?? [], {
        filter,
        searchQuery,
      }),
    [downloads, filter, searchQuery],
  );

  const visibleSelectedDownloadIds = useMemo(() => {
    const visibleIds = new Set(filteredDownloads.map((download) => download.id));
    return selectedDownloadIds.filter((id) => visibleIds.has(id));
  }, [filteredDownloads, selectedDownloadIds]);

  const visibleSelectedDownloads = useMemo(() => {
    const visibleSelectedSet = new Set(visibleSelectedDownloadIds);
    return filteredDownloads.filter((download) =>
      visibleSelectedSet.has(download.id),
    );
  }, [filteredDownloads, visibleSelectedDownloadIds]);

  useEffect(() => {
    const selectionChanged =
      selectedDownloadIds.length !== visibleSelectedDownloadIds.length ||
      selectedDownloadIds.some(
        (id, index) => id !== visibleSelectedDownloadIds[index],
      );

    if (selectionChanged) {
      setSelectedDownloadIds(visibleSelectedDownloadIds);
    }

    if (
      selectedDownloadId &&
      !filteredDownloads.some((download) => download.id === selectedDownloadId)
    ) {
      selectDownload(null);
    }
  }, [
    filteredDownloads,
    selectedDownloadId,
    selectedDownloadIds,
    selectDownload,
    setSelectedDownloadIds,
    visibleSelectedDownloadIds,
  ]);

  const handleToggleSelected = useCallback(async () => {
    if (visibleSelectedDownloads.length === 0) return;

    const tasks = [
      ...visibleSelectedDownloads
        .filter((download) =>
          download.state === 'Downloading' || download.state === 'Queued',
        )
        .map((download) => pauseMut.mutateAsync({ id: download.id })),
      ...visibleSelectedDownloads
        .filter((download) => download.state === 'Paused')
        .map((download) => resumeMut.mutateAsync({ id: download.id })),
    ];

    if (tasks.length === 0) return;
    await Promise.allSettled(tasks);
  }, [pauseMut, resumeMut, visibleSelectedDownloads]);

  const handleRemoveSelected = useCallback(async () => {
    if (visibleSelectedDownloadIds.length === 0) return;

    const selectionSnapshot = {
      ids: [...visibleSelectedDownloadIds],
      activeId: useUiStore.getState().selectedDownloadId,
    };
    const results = await Promise.allSettled(
      selectionSnapshot.ids.map((id) =>
        removeMut.mutateAsync({ id, deleteFiles: false }),
      ),
    );
    const failedIds = selectionSnapshot.ids.filter(
      (_, index) => results[index].status === 'rejected',
    );
    const currentState = useUiStore.getState();
    const unchanged =
      currentState.selectedDownloadId === selectionSnapshot.activeId &&
      currentState.selectedDownloadIds.length === selectionSnapshot.ids.length &&
      currentState.selectedDownloadIds.every(
        (id, index) => id === selectionSnapshot.ids[index],
      );

    if (!unchanged) return;

    if (failedIds.length === 0) {
      clearSelection();
      return;
    }

    setSelectedDownloadIds(failedIds);
    if (
      currentState.selectedDownloadId &&
      !failedIds.includes(currentState.selectedDownloadId)
    ) {
      selectDownload(null);
    }
  }, [
    clearSelection,
    removeMut,
    selectDownload,
    setSelectedDownloadIds,
    visibleSelectedDownloadIds,
  ]);

  useEffect(() => {
    return subscribeShortcutAction((action) => {
      switch (action) {
        case SHORTCUT_ACTIONS.downloadsFocusSearch: {
          const input = document.querySelector<HTMLInputElement>(
            '[data-shortcut-target="downloads-search"]',
          );
          input?.focus();
          input?.select();
          return;
        }
        case SHORTCUT_ACTIONS.downloadsSelectAll:
          setSelectedDownloadIds(filteredDownloads.map((download) => download.id));
          return;
        case SHORTCUT_ACTIONS.downloadsToggleSelected:
          void handleToggleSelected();
          return;
        case SHORTCUT_ACTIONS.downloadsRemoveSelected:
          void handleRemoveSelected();
          return;
        default:
          return;
      }
    });
  }, [
    filteredDownloads,
    handleRemoveSelected,
    handleToggleSelected,
    setSelectedDownloadIds,
  ]);

  return (
    <div className="flex h-full">
      <div className="flex min-w-0 flex-1 flex-col gap-3 p-4">
        <SearchBar value={searchQuery} onChange={setSearchQuery} />
        <FilterBar
          activeFilter={filter}
          onFilterChange={setFilter}
          counts={countByState}
        />
        <ActionsBar />
        <DownloadsTable
          downloads={filteredDownloads}
          downloadsAreFiltered
          isLoading={isLoading}
          filter={filter}
          searchQuery={searchQuery}
        />
      </div>
      <DownloadDetailsPanel />
    </div>
  );
}
