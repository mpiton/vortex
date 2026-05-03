import { Progress } from "@/components/ui/progress";
import { useDownloadStore } from "@/stores/downloadStore";
import type { DownloadView } from "@/types/download";

interface ProgressCellProps {
  download: DownloadView;
}

export function ProgressCell({ download }: ProgressCellProps) {
  const progress = useDownloadStore((s) => s.progressMap[download.id]);

  const raw = progress
    ? progress.totalBytes > 0
      ? Math.round((progress.downloadedBytes / progress.totalBytes) * 100)
      : 0
    : download.progressPercent;
  const percent = Math.min(100, Math.max(0, raw));

  return (
    <div className="flex items-center gap-2">
      <Progress className="flex-1 h-2" value={percent} />
      <span className="text-xs font-mono">{percent}%</span>
    </div>
  );
}
