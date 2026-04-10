import { useTauriEvent } from '@/hooks/useTauriEvent';
import { queryClient } from '@/api/client';
import { downloadQueries } from '@/api/queries';
import type { DownloadIdPayload, DownloadFailedPayload } from '@/types/events';

export function useDownloadEvents(): void {
  const invalidateDownloads = () => {
    queryClient.invalidateQueries({ queryKey: downloadQueries.all() });
  };

  useTauriEvent<DownloadIdPayload>('download-created', invalidateDownloads);
  useTauriEvent<DownloadIdPayload>('download-started', invalidateDownloads);
  useTauriEvent<DownloadIdPayload>('download-paused', invalidateDownloads);
  useTauriEvent<DownloadIdPayload>('download-resumed', invalidateDownloads);
  useTauriEvent<DownloadIdPayload>('download-completed', invalidateDownloads);
  useTauriEvent<DownloadFailedPayload>('download-failed', invalidateDownloads);
  useTauriEvent<DownloadIdPayload>('download-cancelled', invalidateDownloads);
  useTauriEvent<DownloadIdPayload>('download-waiting', invalidateDownloads);
}
