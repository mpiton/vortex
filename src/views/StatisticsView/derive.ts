import type { HistoryView } from "@/types/download";

export interface TypeBreakdownEntry {
  extension: string;
  bytes: number;
  count: number;
}

export interface SpeedPoint {
  date: string;
  avgSpeed: number;
}

export type StatsPeriod = "7d" | "30d" | "all";

const SECONDS_PER_DAY = 86_400;
const UNKNOWN_EXTENSION = "other";

export function extractExtension(fileName: string): string {
  const lastDot = fileName.lastIndexOf(".");
  if (lastDot <= 0 || lastDot === fileName.length - 1) {
    return UNKNOWN_EXTENSION;
  }
  return fileName.slice(lastDot + 1).toLowerCase();
}

export function deriveTypeBreakdown(entries: HistoryView[]): TypeBreakdownEntry[] {
  const totals = new Map<string, { bytes: number; count: number }>();
  for (const entry of entries) {
    const ext = extractExtension(entry.fileName);
    const current = totals.get(ext) ?? { bytes: 0, count: 0 };
    current.bytes += entry.totalBytes;
    current.count += 1;
    totals.set(ext, current);
  }
  return Array.from(totals.entries())
    .map(([extension, value]) => ({ extension, bytes: value.bytes, count: value.count }))
    .sort((a, b) => b.bytes - a.bytes);
}

function toDayKey(epochSeconds: number): string {
  const date = new Date(epochSeconds * 1000);
  const year = date.getUTCFullYear();
  const month = String(date.getUTCMonth() + 1).padStart(2, "0");
  const day = String(date.getUTCDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export function deriveSpeedSeries(entries: HistoryView[]): SpeedPoint[] {
  const buckets = new Map<string, { sum: number; count: number }>();
  for (const entry of entries) {
    const key = toDayKey(entry.completedAt);
    const bucket = buckets.get(key) ?? { sum: 0, count: 0 };
    bucket.sum += entry.avgSpeed;
    bucket.count += 1;
    buckets.set(key, bucket);
  }
  return Array.from(buckets.entries())
    .map(([date, bucket]) => ({
      date,
      avgSpeed: bucket.count === 0 ? 0 : Math.round(bucket.sum / bucket.count),
    }))
    .sort((a, b) => (a.date < b.date ? -1 : 1));
}

export function periodToCutoffSeconds(period: StatsPeriod, nowSeconds: number): number | null {
  if (period === "all") return null;
  const days = period === "7d" ? 7 : 30;
  return nowSeconds - days * SECONDS_PER_DAY;
}

export function filterEntriesByPeriod(
  entries: HistoryView[],
  period: StatsPeriod,
  nowSeconds: number,
): HistoryView[] {
  const cutoff = periodToCutoffSeconds(period, nowSeconds);
  if (cutoff === null) return entries;
  return entries.filter((e) => e.completedAt >= cutoff);
}
