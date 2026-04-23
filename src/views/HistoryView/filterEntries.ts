import type { HistoryView } from '@/types/download';

export type HistoryStatus = 'completed' | 'failed' | 'cancelled';
export type HistoryFilterType = 'all' | 'completed' | 'failed' | 'cancelled';

export function deriveHistoryStatus(_entry: HistoryView): HistoryStatus {
  return 'completed';
}

export function deriveHostname(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return '—';
  }
}

interface FilterOptions {
  filter: HistoryFilterType;
  searchQuery: string;
}

function matchesFilter(entry: HistoryView, filter: HistoryFilterType): boolean {
  if (filter === 'all') return true;
  return deriveHistoryStatus(entry) === filter;
}

function matchesSearch(entry: HistoryView, query: string): boolean {
  if (!query) return true;
  const needle = query.trim().toLowerCase();
  if (!needle) return true;
  return (
    entry.fileName.toLowerCase().includes(needle) ||
    entry.url.toLowerCase().includes(needle) ||
    deriveHostname(entry.url).toLowerCase().includes(needle)
  );
}

export function filterHistoryEntries(
  entries: HistoryView[],
  { filter, searchQuery }: FilterOptions,
): HistoryView[] {
  return entries.filter(
    (entry) => matchesFilter(entry, filter) && matchesSearch(entry, searchQuery),
  );
}
