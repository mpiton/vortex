import { describe, expect, it } from 'vitest';
import type { HistoryView } from '@/types/download';
import {
  deriveSpeedSeries,
  deriveTypeBreakdown,
  extractExtension,
  filterEntriesByPeriod,
  periodToCutoffSeconds,
} from '../derive';

function entry(partial: Partial<HistoryView>): HistoryView {
  return {
    entryId: '1',
    downloadId: '1',
    fileName: 'file.bin',
    url: 'https://example.com/file.bin',
    totalBytes: 0,
    completedAt: 0,
    durationSeconds: 0,
    avgSpeed: 0,
    destinationPath: '/tmp/file.bin',
    ...partial,
  };
}

describe('extractExtension', () => {
  it.each([
    ['movie.mkv', 'mkv'],
    ['archive.tar.gz', 'gz'],
    ['IMAGE.JPG', 'jpg'],
    ['no-extension', 'other'],
    ['.dotfile', 'other'],
    ['ends-with-dot.', 'other'],
  ])('returns %s extension as %s', (input, expected) => {
    expect(extractExtension(input)).toBe(expected);
  });
});

describe('deriveTypeBreakdown', () => {
  it('aggregates totalBytes and count per extension and sorts desc by bytes', () => {
    const result = deriveTypeBreakdown([
      entry({ fileName: 'a.zip', totalBytes: 100 }),
      entry({ fileName: 'b.zip', totalBytes: 200 }),
      entry({ fileName: 'c.mkv', totalBytes: 1_000 }),
      entry({ fileName: 'noext', totalBytes: 50 }),
    ]);
    expect(result).toEqual([
      { extension: 'mkv', bytes: 1_000, count: 1 },
      { extension: 'zip', bytes: 300, count: 2 },
      { extension: 'other', bytes: 50, count: 1 },
    ]);
  });

  it('returns empty array for no entries', () => {
    expect(deriveTypeBreakdown([])).toEqual([]);
  });
});

describe('deriveSpeedSeries', () => {
  it('groups by UTC day and averages avgSpeed', () => {
    const day1Noon = Math.floor(Date.UTC(2026, 0, 10, 12, 0, 0) / 1000);
    const day1Evening = Math.floor(Date.UTC(2026, 0, 10, 22, 0, 0) / 1000);
    const day2 = Math.floor(Date.UTC(2026, 0, 11, 9, 0, 0) / 1000);
    const series = deriveSpeedSeries([
      entry({ completedAt: day1Noon, avgSpeed: 200 }),
      entry({ completedAt: day1Evening, avgSpeed: 400 }),
      entry({ completedAt: day2, avgSpeed: 1_000 }),
    ]);
    expect(series).toEqual([
      { date: '2026-01-10', avgSpeed: 300 },
      { date: '2026-01-11', avgSpeed: 1_000 },
    ]);
  });

  it('returns empty for no entries', () => {
    expect(deriveSpeedSeries([])).toEqual([]);
  });
});

describe('periodToCutoffSeconds', () => {
  it('returns null for all-time', () => {
    expect(periodToCutoffSeconds('all', 1_000_000)).toBeNull();
  });

  it('returns now-7d for 7d', () => {
    expect(periodToCutoffSeconds('7d', 1_000_000)).toBe(1_000_000 - 7 * 86_400);
  });

  it('returns now-30d for 30d', () => {
    expect(periodToCutoffSeconds('30d', 1_000_000)).toBe(1_000_000 - 30 * 86_400);
  });
});

describe('filterEntriesByPeriod', () => {
  const now = Math.floor(Date.UTC(2026, 0, 31, 0, 0, 0) / 1000);

  it('keeps everything for all', () => {
    const entries = [entry({ completedAt: 1 }), entry({ completedAt: now })];
    expect(filterEntriesByPeriod(entries, 'all', now)).toHaveLength(2);
  });

  it('drops entries older than 7d window', () => {
    const inside = now - 3 * 86_400;
    const outside = now - 10 * 86_400;
    const result = filterEntriesByPeriod(
      [entry({ completedAt: inside }), entry({ completedAt: outside })],
      '7d',
      now,
    );
    expect(result).toEqual([entry({ completedAt: inside })]);
  });
});
