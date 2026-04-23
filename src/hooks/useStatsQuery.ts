import { useQueries } from '@tanstack/react-query';
import { tauriInvoke } from '@/api/client';
import { historyQueries, statsQueries } from '@/api/queries';
import type { HistoryView, ModuleStats, StatsView } from '@/types/download';
import type { StatsPeriod } from '@/views/StatisticsView/derive';

const TOP_MODULES_LIMIT = 5;

export interface UseStatsQueryResult {
  stats: StatsView | undefined;
  topModules: ModuleStats[] | undefined;
  history: HistoryView[] | undefined;
  isLoading: boolean;
  error: Error | null;
  refetch: () => Promise<void>;
}

export function useStatsQuery(period: StatsPeriod): UseStatsQueryResult {
  const results = useQueries({
    queries: [
      {
        queryKey: [...statsQueries.overview(), period] as const,
        queryFn: () => tauriInvoke<StatsView>('stats_get', { period }),
        staleTime: 30_000,
      },
      {
        queryKey: [...statsQueries.all(), 'topModules', TOP_MODULES_LIMIT] as const,
        queryFn: () =>
          tauriInvoke<ModuleStats[]>('stats_top_modules', { limit: TOP_MODULES_LIMIT }),
        staleTime: 60_000,
      },
      {
        queryKey: historyQueries.lists(),
        queryFn: () => tauriInvoke<HistoryView[]>('history_list'),
        staleTime: 30_000,
      },
    ],
  });

  const [statsResult, modulesResult, historyResult] = results;
  const isLoading = results.some((r) => r.isLoading);
  const error =
    (statsResult.error as Error | null) ??
    (modulesResult.error as Error | null) ??
    (historyResult.error as Error | null) ??
    null;

  return {
    stats: statsResult.data,
    topModules: modulesResult.data,
    history: historyResult.data,
    isLoading,
    error,
    refetch: async () => {
      await Promise.all(results.map((r) => r.refetch()));
    },
  };
}
