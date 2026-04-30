import { useTranslation } from "react-i18next";
import type { DownloadView } from "@/types/download";
import type { PackageView } from "@/types/package";
import { PackageRow, type PackageRowActions } from "./PackageRow";

interface PackageTreeProps {
  packages: PackageView[];
  expandedId: string | null;
  childrenLoading: boolean;
  childrenError: Error | null;
  childrenById: DownloadView[] | null;
  actions: PackageRowActions;
}

export function PackageTree({
  packages,
  expandedId,
  childrenLoading,
  childrenError,
  childrenById,
  actions,
}: PackageTreeProps) {
  const { t } = useTranslation();

  if (packages.length === 0) {
    return (
      <div
        data-testid="packages-empty"
        className="flex h-32 items-center justify-center text-sm text-muted-foreground"
      >
        {t("packages.empty")}
      </div>
    );
  }

  return (
    <div className="rounded-md border">
      {packages.map((pkg) => {
        const isExpanded = expandedId === pkg.id;
        return (
          <PackageRow
            key={pkg.id}
            pkg={pkg}
            expanded={isExpanded}
            childrenLoading={isExpanded && childrenLoading}
            childrenError={isExpanded ? childrenError : null}
            childDownloads={isExpanded ? childrenById : null}
            actions={actions}
          />
        );
      })}
    </div>
  );
}
