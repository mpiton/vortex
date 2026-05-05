/** Rust emits DownloadId.0 (u64) via serde_json. JavaScript receives it as number.
 *  We type IDs as `number` to match the actual JSON wire format.
 *  Consumers must use `String(payload.id)` when correlating with DTO IDs (which are string). */
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

export interface DownloadWaitingStartedPayload {
  id: number;
  untilUnixMs: number;
  totalSeconds: number;
  reason: string;
}

export interface DownloadWaitingEndedPayload {
  id: number;
  expiredNaturally: boolean;
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

export interface ClipboardUrlDetectedPayload {
  urls: string[];
}

export interface ClipboardMonitoringChangedPayload {
  enabled: boolean;
}

export type TauriEventMap = {
  "download-created": DownloadIdPayload;
  "download-started": DownloadIdPayload;
  "download-paused": DownloadIdPayload;
  "download-resumed": DownloadIdPayload;
  "download-resumed-from-wait": DownloadIdPayload;
  "download-completed": DownloadIdPayload;
  "download-failed": DownloadFailedPayload;
  "download-retrying": DownloadRetryingPayload;
  "download-waiting": DownloadIdPayload;
  "download-waiting-started": DownloadWaitingStartedPayload;
  "download-waiting-ended": DownloadWaitingEndedPayload;
  "download-checking": DownloadIdPayload;
  "download-cancelled": DownloadIdPayload;
  "download-removed": DownloadIdPayload;
  "download-extracting": DownloadIdPayload;
  "download-progress": DownloadProgressPayload;
  "segment-started": SegmentPayload;
  "segment-completed": SegmentPayload;
  "segment-failed": SegmentFailedPayload;
  "plugin-loaded": PluginLoadedPayload;
  "plugin-unloaded": PluginUnloadedPayload;
  "package-created": PackageCreatedPayload;
  "clipboard-url-detected": ClipboardUrlDetectedPayload;
  "clipboard-monitoring-changed": ClipboardMonitoringChangedPayload;
};

export type TauriEventName = keyof TauriEventMap;
