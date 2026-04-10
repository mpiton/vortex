import type { DownloadDetailView } from '@/types/download';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';

interface IntegritySectionProps {
  download: DownloadDetailView;
}

export function IntegritySection({ download }: IntegritySectionProps) {
  const hasChecksum = download.checksumExpected !== null;

  return (
    <section className="space-y-3">
      <h3 className="text-sm font-semibold">Integrity</h3>
      <div className="space-y-2 text-xs">
        <div>
          <p className="text-muted-foreground">Algorithm</p>
          <p className="font-mono">SHA-256</p>
        </div>
        <div>
          <p className="text-muted-foreground">Expected Hash</p>
          {hasChecksum ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <p className="font-mono truncate max-w-full cursor-default">
                  {download.checksumExpected}
                </p>
              </TooltipTrigger>
              <TooltipContent>
                <p className="max-w-[400px] break-all">{download.checksumExpected}</p>
              </TooltipContent>
            </Tooltip>
          ) : (
            <p className="font-mono">—</p>
          )}
        </div>
        <div>
          <p className="text-muted-foreground">Status</p>
          <p className="font-mono">{hasChecksum ? 'Pending' : '—'}</p>
        </div>
      </div>
    </section>
  );
}
