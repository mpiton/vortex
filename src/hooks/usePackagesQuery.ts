import { useQuery } from '@tanstack/react-query';
import { tauriInvoke } from '@/api/client';
import { packageQueries } from '@/api/queries';
import type { DownloadView } from '@/types/download';
import type { PackageListFilter, PackageView } from '@/types/package';

export function usePackagesQuery(filter?: PackageListFilter) {
  return useQuery<PackageView[], Error>({
    queryKey: filter ? packageQueries.list(filter) : packageQueries.lists(),
    queryFn: () =>
      tauriInvoke<PackageView[]>('package_list', {
        sourceType: filter?.sourceType,
        nameQ: filter?.nameQ,
      }),
    staleTime: 30_000,
  });
}

export function usePackageDownloadsQuery(packageId: string | null) {
  return useQuery<DownloadView[], Error>({
    queryKey: packageId ? packageQueries.downloads(packageId) : ['packages', 'downloads', 'none'],
    queryFn: () =>
      tauriInvoke<DownloadView[]>('package_list_downloads', {
        id: packageId,
      }),
    enabled: packageId !== null,
    staleTime: 10_000,
  });
}
