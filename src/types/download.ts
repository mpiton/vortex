export type DownloadState =
  | 'Queued'
  | 'Downloading'
  | 'Paused'
  | 'Waiting'
  | 'Retry'
  | 'Error'
  | 'Completed'
  | 'Checking'
  | 'Extracting';

export type SortField = 'name' | 'filename' | 'size' | 'filesize' | 'progress' | 'speed' | 'state' | 'status';
export type SortDirection = 'asc' | 'ascending' | 'desc' | 'descending';

export interface DownloadFilter {
  filterState?: DownloadState;
  search?: string;
  sortField?: SortField;
  sortDirection?: SortDirection;
  limit?: number;
  offset?: number;
}

export interface SegmentView {
  id: number;
  startByte: number;
  endByte: number;
  downloadedBytes: number;
  state: 'Pending' | 'Downloading' | 'Completed' | 'Error';
}

export interface DownloadView {
  id: string;
  fileName: string;
  url: string;
  sourceHostname: string;
  state: DownloadState;
  progressPercent: number;
  speedBytesPerSec: number;
  downloadedBytes: number;
  totalBytes: number | null;
  etaSeconds: number | null;
  segmentsActive: number;
  segmentsTotal: number;
  moduleName: string | null;
  accountName: string | null;
  errorMessage?: string | null;
  createdAt: number;
}

export interface DownloadDetailView {
  id: string;
  fileName: string;
  url: string;
  sourceHostname: string;
  state: DownloadState;
  progressPercent: number;
  speedBytesPerSec: number;
  downloadedBytes: number;
  totalBytes: number | null;
  etaSeconds: number | null;
  segments: SegmentView[];
  checksumExpected: string | null;
  checksumComputed: string | null;
  checksumAlgorithm: string | null;
  destinationPath: string;
  moduleName: string | null;
  accountName: string | null;
  resumeSupported: boolean;
  retryCount: number;
  maxRetries: number;
  createdAt: number;
  updatedAt: number;
}

export type VerifyChecksumOutcome = 'verified' | 'mismatch' | 'noExpectedChecksum';

export interface PluginView {
  name: string;
  version: string;
  description: string;
  author: string;
  category: string;
  enabled: boolean;
}

export interface HistoryView {
  // Backend emits u64 as a string to survive JS number precision.
  entryId: string;
  downloadId: string;
  fileName: string;
  url: string;
  totalBytes: number;
  completedAt: number;
  durationSeconds: number;
  avgSpeed: number;
  destinationPath: string;
}

export interface DailyVolume {
  date: string;
  bytes: number;
  count: number;
}

export interface HostStats {
  hostname: string;
  totalBytes: number;
  downloadCount: number;
}

export interface StatsView {
  totalDownloadedBytes: number;
  totalFiles: number;
  avgSpeed: number;
  peakSpeed: number;
  successRate: number;
  dailyVolumes: DailyVolume[];
  topHosts: HostStats[];
}

export type StatsPeriod = '7d' | '30d' | 'all';

export interface ModuleStats {
  moduleName: string;
  downloadCount: number;
  totalBytes: number;
}
