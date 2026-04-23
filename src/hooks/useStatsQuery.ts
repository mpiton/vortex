import { useQueries } from '@tanstack/react-query';
import { tauriInvoke } from '@/api/client';
import { historyQueries, statsQueries } from '@/api/queries';
import type { HistoryView, ModuleStats, StatsView } from '@/types/download';
import { periodToCutoffSeconds, type StatsPeriod } from '@/views/StatisticsView/derive';

const TOP_MODULES_LIMIT = 5;
const HISTORY_PAGE_SIZE = 500;

export interface QueryStatusFlags {
  isLoading: boolean;
  isError: boolean;
}

export interface UseStatsQueryResult {
  stats: StatsView | undefined;
  topModules: ModuleStats[] | undefined;
  history: HistoryView[] | undefined;
  isLoading: boolean;
  error: Error | null;
  statsStatus: QueryStatusFlags;
  topModulesStatus: QueryStatusFlags;
  historyStatus: QueryStatusFlags;
  refetch: () => Promise<void>;
}

function nowSeconds(): number {
  return Math.floor(Date.now() / 1000);
}

export function useStatsQuery(period: StatsPeriod): UseStatsQueryResult {
  const dateFrom = periodToCutoffSeconds(period, nowSeconds());
  const historyArgs: Record<string, unknown> = { limit: HISTORY_PAGE_SIZE };
  if (dateFrom !== null) historyArgs.dateFrom = dateFrom;

  const results = useQueries({
    queries: [
      {
        queryKey: [...statsQueries.overview(), period] as const,
        queryFn: () => tauriInvoke<StatsView>('stats_get', { period }),
        staleTime: 30_000,
      },
      {
        queryKey: [...statsQueries.overview(), 'topModules', TOP_MODULES_LIMIT] as const,
        queryFn: () =>
          tauriInvoke<ModuleStats[]>('stats_top_modules', { limit: TOP_MODULES_LIMIT }),
        staleTime: 60_000,
      },
      {
        queryKey: [...historyQueries.lists(), 'period', period] as const,
        queryFn: () => tauriInvoke<HistoryView[]>('history_list', historyArgs),
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
    statsStatus: { isLoading: statsResult.isLoading, isError: statsResult.isError },
    topModulesStatus: { isLoading: modulesResult.isLoading, isError: modulesResult.isError },
    historyStatus: { isLoading: historyResult.isLoading, isError: historyResult.isError },
    refetch: async () => {
      const outcomes = await Promise.all(results.map((r) => r.refetch()));
      const firstError = outcomes.find((o) => o.error)?.error;
      if (firstError) throw firstError;
    },
  };
}
