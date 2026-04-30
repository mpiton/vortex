import { useTranslation } from "react-i18next";
import { GripVertical } from "lucide-react";
import { Progress } from "@/components/ui/progress";
import { formatBytes, formatEta, formatSpeed } from "@/lib/format";
import type { DownloadView } from "@/types/download";

interface PackageDownloadRowProps {
  download: DownloadView;
  packageId: string;
  onDragStart: (download: DownloadView, fromPackageId: string) => void;
  onDragEnd: () => void;
}

export function PackageDownloadRow({
  download,
  packageId,
  onDragStart,
  onDragEnd,
}: PackageDownloadRowProps) {
  const { t } = useTranslation();
  return (
    <div
      data-testid={`package-download-row-${download.id}`}
      draggable
      onDragStart={(e) => {
        e.dataTransfer.effectAllowed = "move";
        e.dataTransfer.setData("application/x-vortex-download", download.id);
        e.dataTransfer.setData("application/x-vortex-source-package", packageId);
        onDragStart(download, packageId);
      }}
      onDragEnd={onDragEnd}
      className="flex items-center gap-3 border-t bg-muted/30 px-4 py-2 text-sm hover:bg-muted/50"
    >
      <GripVertical
        className="h-4 w-4 text-muted-foreground"
        aria-label={t("packages.drag.downloadHandleAriaLabel")}
      />
      <span className="min-w-0 flex-1 truncate font-medium">{download.fileName}</span>
      <span className="text-xs text-muted-foreground">{download.state}</span>
      <span className="text-xs text-muted-foreground">{formatBytes(download.totalBytes)}</span>
      <span className="text-xs text-muted-foreground">{formatSpeed(download.speedBytesPerSec)}</span>
      <span className="text-xs text-muted-foreground">{formatEta(download.etaSeconds)}</span>
      <Progress value={download.progressPercent} className="h-1.5 w-32" />
    </div>
  );
}
