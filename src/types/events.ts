export interface DownloadIdPayload {
  id: number;
}

export interface DownloadFailedPayload {
  id: number;
  error: string;
}

export interface DownloadRetryingPayload {
  id: number;
  attempt: number;
}

export interface DownloadProgressPayload {
  id: number;
  downloadedBytes: number;
  totalBytes: number;
}

export interface SegmentPayload {
  downloadId: number;
  segmentId: number;
}

export interface SegmentFailedPayload {
  downloadId: number;
  segmentId: number;
  error: string;
}

export interface PluginLoadedPayload {
  name: string;
  version: string;
}

export interface PluginUnloadedPayload {
  name: string;
}

export interface PackageCreatedPayload {
  id: string;
  name: string;
}

export type TauriEventMap = {
  'download-created': DownloadIdPayload;
  'download-started': DownloadIdPayload;
  'download-paused': DownloadIdPayload;
  'download-resumed': DownloadIdPayload;
  'download-resumed-from-wait': DownloadIdPayload;
  'download-completed': DownloadIdPayload;
  'download-failed': DownloadFailedPayload;
  'download-retrying': DownloadRetryingPayload;
  'download-waiting': DownloadIdPayload;
  'download-checking': DownloadIdPayload;
  'download-cancelled': DownloadIdPayload;
  'download-extracting': DownloadIdPayload;
  'download-progress': DownloadProgressPayload;
  'segment-started': SegmentPayload;
  'segment-completed': SegmentPayload;
  'segment-failed': SegmentFailedPayload;
  'plugin-loaded': PluginLoadedPayload;
  'plugin-unloaded': PluginUnloadedPayload;
  'package-created': PackageCreatedPayload;
};

export type TauriEventName = keyof TauriEventMap;
