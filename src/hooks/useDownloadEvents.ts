import { useTauriEvent } from "@/hooks/useTauriEvent";
import { queryClient } from "@/api/client";
import { downloadQueries } from "@/api/queries";
import { useDownloadStore } from "@/stores/downloadStore";
import type {
  DownloadIdPayload,
  DownloadFailedPayload,
  DownloadRetryingPayload,
  DownloadWaitingStartedPayload,
  DownloadWaitingEndedPayload,
} from "@/types/events";

export function useDownloadEvents(): void {
  const invalidateDownloads = () => {
    queryClient.invalidateQueries({ queryKey: downloadQueries.all() });
  };

  useTauriEvent<DownloadIdPayload>("download-created", invalidateDownloads);
  useTauriEvent<DownloadIdPayload>("download-started", invalidateDownloads);
  useTauriEvent<DownloadIdPayload>("download-paused", invalidateDownloads);
  useTauriEvent<DownloadIdPayload>("download-resumed", invalidateDownloads);
  useTauriEvent<DownloadIdPayload>("download-resumed-from-wait", invalidateDownloads);
  useTauriEvent<DownloadIdPayload>("download-completed", invalidateDownloads);
  useTauriEvent<DownloadFailedPayload>("download-failed", invalidateDownloads);
  useTauriEvent<DownloadRetryingPayload>("download-retrying", invalidateDownloads);
  useTauriEvent<DownloadIdPayload>("download-cancelled", invalidateDownloads);
  useTauriEvent<DownloadIdPayload>("download-waiting", invalidateDownloads);
  useTauriEvent<DownloadIdPayload>("download-checking", invalidateDownloads);
  useTauriEvent<DownloadIdPayload>("download-removed", invalidateDownloads);
  useTauriEvent<DownloadIdPayload>("download-extracting", invalidateDownloads);
  // The accompanying `download-waiting` / `download-resumed-from-wait`
  // events already trigger query invalidation; these two only sync the
  // per-row wait ticket into the store for the countdown UI.
  useTauriEvent<DownloadWaitingStartedPayload>("download-waiting-started", (payload) => {
    useDownloadStore.getState().setWait(String(payload.id), {
      untilUnixMs: payload.untilUnixMs,
      totalSeconds: payload.totalSeconds,
      reason: payload.reason,
    });
  });
  useTauriEvent<DownloadWaitingEndedPayload>("download-waiting-ended", (payload) => {
    useDownloadStore.getState().clearWait(String(payload.id));
  });
}
