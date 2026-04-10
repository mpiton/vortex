import { describe, it, expect } from 'vitest';
import { downloadQueries, pluginQueries, historyQueries, statsQueries } from '@/api/queries';
import type { DownloadFilter } from '@/types/download';

describe('downloadQueries', () => {
  it('should return base key for all()', () => {
    expect(downloadQueries.all()).toEqual(['downloads']);
  });

  it('should return list key for lists()', () => {
    expect(downloadQueries.lists()).toEqual(['downloads', 'list']);
  });

  it('should return list key with filters when provided', () => {
    const filter: DownloadFilter = { filterState: 'Downloading' };
    const key = downloadQueries.list(filter);
    expect(key).toEqual(['downloads', 'list', filter]);
  });

  it('should return base lists key when no filter provided', () => {
    const key = downloadQueries.list();
    expect(key).toEqual(['downloads', 'list']);
  });

  it('should return detail base key for details()', () => {
    expect(downloadQueries.details()).toEqual(['downloads', 'detail']);
  });

  it('should return detail key with id for detail()', () => {
    expect(downloadQueries.detail('42')).toEqual(['downloads', 'detail', '42']);
  });
});

describe('pluginQueries', () => {
  it('should return base key for all()', () => {
    expect(pluginQueries.all()).toEqual(['plugins']);
  });

  it('should return list key for lists()', () => {
    expect(pluginQueries.lists()).toEqual(['plugins', 'list']);
  });

  it('should return list key for list()', () => {
    expect(pluginQueries.list()).toEqual(['plugins', 'list']);
  });
});

describe('historyQueries', () => {
  it('should return base key for all()', () => {
    expect(historyQueries.all()).toEqual(['history']);
  });

  it('should return list key for list()', () => {
    expect(historyQueries.list()).toEqual(['history', 'list']);
  });
});

describe('statsQueries', () => {
  it('should return base key for all()', () => {
    expect(statsQueries.all()).toEqual(['stats']);
  });

  it('should return overview key for overview()', () => {
    expect(statsQueries.overview()).toEqual(['stats', 'overview']);
  });
});
