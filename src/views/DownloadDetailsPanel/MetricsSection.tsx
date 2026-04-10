import { useDownloadStore } from '@/stores/downloadStore';
import { formatSpeed, formatBytes, formatEta } from '@/lib/format';
import { Progress } from '@/components/ui/progress';
import type { DownloadDetailView } from '@/types/download';

interface MetricsSectionProps {
  download: DownloadDetailView;
}

export function MetricsSection({ download }: MetricsSectionProps) {
  const progress = useDownloadStore((s) => s.progressMap[download.id]);

  const speed = progress?.speedBytesPerSec ?? download.speedBytesPerSec;
  const downloaded = progress?.downloadedBytes ?? download.downloadedBytes;
  const total = progress?.totalBytes ?? download.totalBytes;
  const progressPercent = Math.min(
    100,
    Math.max(0, total && total > 0 ? (downloaded / total) * 100 : download.progressPercent),
  );

  const speedIsHigh = speed > 1_048_576;
  const remaining = (total ?? 0) - downloaded;
  const eta = speed > 0 && remaining > 0 ? remaining / speed : download.etaSeconds;

  return (
    <section className="space-y-3">
      <h3 className="text-sm font-semibold">Metrics</h3>
      <div className="grid grid-cols-2 gap-2 text-xs">
      <div className="rounded bg-background p-2">
        <div className="text-muted-foreground">Speed</div>
        <div className={`font-mono font-semibold ${speedIsHigh ? 'text-green-600' : ''}`}>
          {formatSpeed(speed)}
        </div>
      </div>

      <div className="rounded bg-background p-2">
        <div className="text-muted-foreground">ETA</div>
        <div className="font-mono font-semibold">{formatEta(eta)}</div>
      </div>

      <div className="rounded bg-background p-2">
        <div className="text-muted-foreground">Downloaded</div>
        <div className="font-mono font-semibold">{formatBytes(downloaded)}</div>
      </div>

      <div className="rounded bg-background p-2">
        <div className="text-muted-foreground">Total</div>
        <div className="font-mono font-semibold">{formatBytes(total)}</div>
      </div>

      <div className="col-span-2 rounded bg-background p-2">
        <div className="text-muted-foreground">Connections</div>
        <div className="font-mono font-semibold">
          {download.segments.filter((s) => s.state === 'Downloading').length}
        </div>
      </div>

      <div className="col-span-2 rounded bg-background p-2">
        <div className="text-muted-foreground mb-1">Progress</div>
        <Progress value={progressPercent} />
      </div>
    </div>
    </section>
  );
}
