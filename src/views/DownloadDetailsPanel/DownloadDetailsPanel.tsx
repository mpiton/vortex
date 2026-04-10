import { X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { useUiStore } from '@/stores/uiStore';
import { useDownloadDetail } from '@/hooks/useDownloadDetail';
import { FileInfoSection } from './FileInfoSection';
import { MetricsSection } from './MetricsSection';
import { SegmentVisualization } from './SegmentVisualization';
import { SpeedSparkline } from './SpeedSparkline';
import { SourceInfoSection } from './SourceInfoSection';
import { IntegritySection } from './IntegritySection';
import { ModuleSection } from './ModuleSection';
import { LogsSection } from './LogsSection';

export function DownloadDetailsPanel() {
  const selectedDownloadId = useUiStore((s) => s.selectedDownloadId);
  const detailsPanelOpen = useUiStore((s) => s.detailsPanelOpen);
  const setDetailsPanelOpen = useUiStore((s) => s.setDetailsPanelOpen);

  if (!detailsPanelOpen) return null;

  if (!selectedDownloadId) {
    return (
      <aside className="w-80 shrink-0 border-l bg-muted/30">
        <div className="flex items-center justify-between border-b px-4 py-2">
          <h2 className="text-sm font-semibold">Details</h2>
          <Button variant="ghost" size="icon" className="h-7 w-7" aria-label="Close details panel" onClick={() => setDetailsPanelOpen(false)}>
            <X className="size-3.5" />
          </Button>
        </div>
        <p className="p-4 text-center text-sm text-muted-foreground">
          Select a download to view details
        </p>
      </aside>
    );
  }

  return (
    <DownloadDetailContent
      downloadId={selectedDownloadId}
      onClose={() => setDetailsPanelOpen(false)}
    />
  );
}

function DownloadDetailContent({
  downloadId,
  onClose,
}: {
  downloadId: string;
  onClose: () => void;
}) {
  const { data: detail, isLoading } = useDownloadDetail(downloadId);

  if (isLoading) {
    return (
      <aside className="w-80 shrink-0 border-l bg-muted/30 p-4">
        <div className="animate-pulse space-y-4">
          {Array.from({ length: 5 }).map((_, i) => (
            <div key={i} className="h-16 rounded bg-muted" />
          ))}
        </div>
      </aside>
    );
  }

  if (!detail) {
    return (
      <aside className="w-80 shrink-0 border-l bg-muted/30 p-4">
        <p className="text-sm text-muted-foreground">Download not found</p>
      </aside>
    );
  }

  return (
    <aside className="w-80 shrink-0 border-l bg-muted/30 overflow-y-auto">
      <div className="flex items-center justify-between border-b px-4 py-2">
        <h2 className="text-sm font-semibold">Details</h2>
        <Button variant="ghost" size="icon" className="h-7 w-7" aria-label="Close details panel" onClick={onClose}>
          <X className="size-3.5" />
        </Button>
      </div>
      <div className="space-y-4 p-4">
        <FileInfoSection download={detail} />
        <Separator />
        <MetricsSection download={detail} />
        <Separator />
        <SegmentVisualization segments={detail.segments} totalBytes={detail.totalBytes} />
        <Separator />
        <SpeedSparkline downloadId={downloadId} />
        <Separator />
        <SourceInfoSection download={detail} />
        <Separator />
        <IntegritySection download={detail} />
        <Separator />
        <ModuleSection download={detail} />
        <Separator />
        <LogsSection downloadId={downloadId} />
      </div>
    </aside>
  );
}
