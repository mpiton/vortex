import { useState } from 'react';
import { useTauriQuery } from '@/api/hooks';
import { downloadQueries } from '@/api/queries';
import { useDownloadProgress } from '@/hooks/useDownloadProgress';
import type { DownloadView } from '@/types/download';
import type { FilterType } from './types';
import { SearchBar } from './SearchBar';
import { FilterBar } from './FilterBar';
import { ActionsBar } from './ActionsBar';
import { DownloadsTable } from './DownloadsTable';
import { DownloadDetailsPanel } from '../DownloadDetailsPanel';

export function DownloadsView() {
  const [filter, setFilter] = useState<FilterType>('all');
  const [searchQuery, setSearchQuery] = useState('');

  useDownloadProgress();

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
          downloads={downloads ?? []}
          isLoading={isLoading}
          filter={filter}
          searchQuery={searchQuery}
        />
      </div>
      <DownloadDetailsPanel />
    </div>
  );
}
