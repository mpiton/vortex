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
import { DownloadsTable } from './DownloadsTable';
import { DownloadDetailsPanel } from '../DownloadDetailsPanel';

const INVALIDATE_KEYS = [
  downloadQueries.lists(),
  downloadQueries.countByState(),
] as const;

export function DownloadsView() {
  const [filter, setFilter] = useState<FilterType>('all');
  const [searchQuery, setSearchQuery] = useState('');
  const selectedDownloadIds = useUiStore((s) => s.selectedDownloadIds);
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
      (downloads ?? []).filter((download) => {
        if (
          filter === 'active' &&
          !['Downloading', 'Queued'].includes(download.state)
        ) {
          return false;
        }

        if (filter === 'queued' && download.state !== 'Queued') {
          return false;
        }

        if (filter === 'done' && download.state !== 'Completed') {
          return false;
        }

        if (
          filter === 'failed' &&
          !['Error', 'Retry'].includes(download.state)
        ) {
          return false;
        }

        if (!searchQuery) return true;

        const query = searchQuery.toLowerCase();
        let hostname = '';
        try {
          hostname = new URL(download.url).hostname.toLowerCase();
        } catch {
          hostname = '';
        }

        return (
          download.fileName.toLowerCase().includes(query) ||
          download.url.toLowerCase().includes(query) ||
          hostname.includes(query)
        );
      }),
    [downloads, filter, searchQuery],
  );

  const handleToggleSelected = useCallback(async () => {
    if (selectedDownloadIds.length === 0) return;

    const selectedSet = new Set(selectedDownloadIds);
    const selectedDownloads = (downloads ?? []).filter((download) =>
      selectedSet.has(download.id),
    );

    const tasks = [
      ...selectedDownloads
        .filter((download) =>
          download.state === 'Downloading' || download.state === 'Queued',
        )
        .map((download) => pauseMut.mutateAsync({ id: download.id })),
      ...selectedDownloads
        .filter((download) => download.state === 'Paused')
        .map((download) => resumeMut.mutateAsync({ id: download.id })),
    ];

    if (tasks.length === 0) return;
    await Promise.allSettled(tasks);
  }, [downloads, pauseMut, resumeMut, selectedDownloadIds]);

  const handleRemoveSelected = useCallback(async () => {
    if (selectedDownloadIds.length === 0) return;

    const snapshot = [...selectedDownloadIds];
    const results = await Promise.allSettled(
      snapshot.map((id) => removeMut.mutateAsync({ id, deleteFiles: false })),
    );
    const failedIds = snapshot.filter((_, index) => results[index].status === 'rejected');
    const currentIds = useUiStore.getState().selectedDownloadIds;
    const unchanged =
      currentIds.length === snapshot.length &&
      currentIds.every((id, index) => id === snapshot[index]);

    if (!unchanged) return;

    if (failedIds.length === 0) {
      clearSelection();
      return;
    }

    setSelectedDownloadIds(failedIds);
  }, [clearSelection, removeMut, selectedDownloadIds, setSelectedDownloadIds]);

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
