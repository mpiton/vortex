import { formatSpeed } from '@/lib/format';
import { useDownloadStore } from '@/stores/downloadStore';

interface SpeedCellProps {
  downloadId: string;
}

export function SpeedCell({ downloadId }: SpeedCellProps) {
  const progress = useDownloadStore((s) => s.progressMap[downloadId]);
  const speed = progress?.speedBytesPerSec ?? 0;
  const mb = speed / 1024 / 1024;

  const colorClass =
    mb > 10
      ? 'text-green-600 dark:text-green-400'
      : mb > 1
        ? 'text-blue-600 dark:text-blue-400'
        : 'text-muted-foreground';

  return (
    <span className={`text-xs font-mono ${colorClass}`}>
      {formatSpeed(speed)}
    </span>
  );
}
