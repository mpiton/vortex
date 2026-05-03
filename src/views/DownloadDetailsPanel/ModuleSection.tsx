import type { DownloadDetailView } from "@/types/download";
import { Badge } from "@/components/ui/badge";

interface ModuleSectionProps {
  download: DownloadDetailView;
}

export function ModuleSection({ download }: ModuleSectionProps) {
  return (
    <section className="space-y-3">
      <h3 className="text-sm font-semibold">Module and Account</h3>
      <div className="space-y-2 text-xs">
        <div>
          <p className="text-muted-foreground">Module</p>
          <div className="mt-1">
            {download.moduleName !== null ? (
              <Badge variant="secondary">{download.moduleName}</Badge>
            ) : (
              <p className="font-mono">—</p>
            )}
          </div>
        </div>
        {download.accountName !== null && (
          <div>
            <p className="text-muted-foreground">Account</p>
            <p className="font-mono">{download.accountName}</p>
          </div>
        )}
      </div>
    </section>
  );
}
