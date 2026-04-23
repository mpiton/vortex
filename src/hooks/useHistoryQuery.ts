import { useQuery } from '@tanstack/react-query';
import { tauriInvoke } from '@/api/client';
import { historyQueries } from '@/api/queries';
import type { HistoryView } from '@/types/download';

export interface UseHistoryQueryOptions {
  searchQuery?: string;
}

export function useHistoryQuery(options: UseHistoryQueryOptions = {}) {
  const searchQuery = options.searchQuery?.trim() ?? '';
  const hasSearch = searchQuery.length > 0;

  return useQuery<HistoryView[], Error>({
    queryKey: hasSearch
      ? [...historyQueries.lists(), 'search', searchQuery]
      : historyQueries.lists(),
    queryFn: () => {
      if (hasSearch) {
        return tauriInvoke<HistoryView[]>('history_search', { q: searchQuery });
      }
      return tauriInvoke<HistoryView[]>('history_list');
    },
    staleTime: 30_000,
  });
}
