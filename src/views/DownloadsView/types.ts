import type { DownloadState } from '@/types/download';

export type FilterType = 'all' | 'active' | 'queued' | 'done' | 'failed';

export interface FilterConfig {
  type: FilterType;
  label: string;
  states?: DownloadState[];
}
