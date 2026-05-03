import type { DownloadDetailView } from "@/types/download";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

interface SourceInfoSectionProps {
  download: DownloadDetailView;
}

function getHostname(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return url;
  }
}

function getProtocol(url: string): string {
  try {
    return new URL(url).protocol.replace(":", "").toUpperCase();
  } catch {
    const parts = url.split("://");
    return parts.length > 1 ? parts[0].toUpperCase() : "UNKNOWN";
  }
}

export function SourceInfoSection({ download }: SourceInfoSectionProps) {
  const hostname = getHostname(download.url);
  const protocol = getProtocol(download.url);

  return (
    <section className="space-y-3">
      <h3 className="text-sm font-semibold">Source</h3>
      <div className="space-y-2 text-xs">
        <div>
          <p className="text-muted-foreground">Host</p>
          <p className="font-mono">{hostname}</p>
        </div>
        <div>
          <p className="text-muted-foreground">URL</p>
          <Tooltip>
            <TooltipTrigger asChild>
              <p className="font-mono truncate max-w-full cursor-default">{download.url}</p>
            </TooltipTrigger>
            <TooltipContent>
              <p className="max-w-[400px] break-all">{download.url}</p>
            </TooltipContent>
          </Tooltip>
        </div>
        <div>
          <p className="text-muted-foreground">Protocol</p>
          <p className="font-mono">{protocol}</p>
        </div>
        <div>
          <p className="text-muted-foreground">Resume Supported</p>
          <p className="font-mono">{download.resumeSupported ? "Yes" : "No"}</p>
        </div>
      </div>
    </section>
  );
}
