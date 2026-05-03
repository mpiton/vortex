import { formatEta } from "@/lib/format";
import { useDownloadStore } from "@/stores/downloadStore";

interface EtaCellProps {
  downloadId: string;
}

export function EtaCell({ downloadId }: EtaCellProps) {
  const progress = useDownloadStore((s) => s.progressMap[downloadId]);

  let eta: number | null = null;
  if (progress && progress.speedBytesPerSec > 0) {
    const remainingBytes = Math.max(0, progress.totalBytes - progress.downloadedBytes);
    eta = remainingBytes / progress.speedBytesPerSec;
  }

  return (
    <span className="text-xs text-muted-foreground font-mono">
      {eta !== null ? formatEta(eta) : "\u2014"}
    </span>
  );
}
