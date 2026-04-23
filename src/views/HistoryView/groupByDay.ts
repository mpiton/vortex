import type { HistoryView } from '@/types/download';

export interface HistoryDayGroup {
  dayKey: string;
  completedAt: number;
  entries: HistoryView[];
}

function toDayKey(unixSeconds: number): string {
  const date = new Date(unixSeconds * 1000);
  const y = date.getFullYear();
  const m = `${date.getMonth() + 1}`.padStart(2, '0');
  const d = `${date.getDate()}`.padStart(2, '0');
  return `${y}-${m}-${d}`;
}

export function groupByDay(entries: HistoryView[]): HistoryDayGroup[] {
  const bucket = new Map<string, HistoryDayGroup>();

  for (const entry of entries) {
    const dayKey = toDayKey(entry.completedAt);
    const existing = bucket.get(dayKey);
    if (existing) {
      existing.entries.push(entry);
      if (entry.completedAt > existing.completedAt) {
        existing.completedAt = entry.completedAt;
      }
    } else {
      bucket.set(dayKey, {
        dayKey,
        completedAt: entry.completedAt,
        entries: [entry],
      });
    }
  }

  const groups = Array.from(bucket.values());
  groups.sort((a, b) => b.completedAt - a.completedAt);
  for (const group of groups) {
    group.entries.sort((a, b) => b.completedAt - a.completedAt);
  }
  return groups;
}
