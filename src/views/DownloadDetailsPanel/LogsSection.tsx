import { useTauriQuery } from '@/api/hooks';
import { ScrollArea } from '@/components/ui/scroll-area';

interface LogsSectionProps {
  downloadId: string;
}

export function LogsSection({ downloadId }: LogsSectionProps) {
  const { data: logs, isLoading } = useTauriQuery<string[]>(
    'query_download_logs',
    { id: downloadId, limit: 20 },
    { queryKey: ['query_download_logs', downloadId], staleTime: 2000 },
  );

  return (
    <section className="space-y-3">
      <h3 className="text-sm font-semibold">Logs</h3>
      <ScrollArea className="h-48 rounded border bg-background p-2">
      {isLoading ? (
        <div className="text-muted-foreground text-xs">Loading logs...</div>
      ) : !logs || logs.length === 0 ? (
        <div className="text-muted-foreground text-xs">No logs</div>
      ) : (
        logs.map((line, index) => (
          <div
            key={index}
            className="font-mono text-xs text-muted-foreground whitespace-pre-wrap break-words"
          >
            {line}
          </div>
        ))
      )}
      </ScrollArea>
    </section>
  );
}
