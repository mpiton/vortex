import { useTauriQuery } from '@/api/hooks';
import { downloadQueries } from '@/api/queries';
import type { DownloadDetailView } from '@/types/download';

export function useDownloadDetail(downloadId: string) {
  return useTauriQuery<DownloadDetailView>(
    'query_download_detail',
    { id: downloadId },
    { queryKey: downloadQueries.detail(downloadId), staleTime: 500 },
  );
}
