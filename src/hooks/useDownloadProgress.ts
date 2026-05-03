import { useTauriEvent } from "@/hooks/useTauriEvent";
import { useDownloadStore } from "@/stores/downloadStore";
import type { DownloadProgressPayload } from "@/types/events";

export function useDownloadProgress(): void {
  useTauriEvent<DownloadProgressPayload>("download-progress", (payload) => {
    useDownloadStore
      .getState()
      .updateProgress(String(payload.id), payload.downloadedBytes, payload.totalBytes);
  });
}
