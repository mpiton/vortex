import { useTauriQuery } from "@/api/hooks";
import type { PackageSummary } from "@/types/package";

export function usePackageByExternalId(externalId: string | undefined) {
  return useTauriQuery<PackageSummary | null>(
    "package_find_by_external_id",
    externalId ? { externalId } : undefined,
    {
      queryKey: ["package_find_by_external_id", { externalId }],
      enabled: !!externalId,
      staleTime: 30_000,
    },
  );
}
