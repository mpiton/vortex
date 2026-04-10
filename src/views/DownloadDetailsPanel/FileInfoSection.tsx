import type { DownloadDetailView } from '@/types/download';
import { formatBytes } from '@/lib/format';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';

interface FileInfoSectionProps {
  download: DownloadDetailView;
}

function getMimeType(fileName: string): string {
  const ext = fileName.split('.').pop()?.toLowerCase();
  const mimeMap: Record<string, string> = {
    mp4: 'video/mp4',
    mkv: 'video/x-matroska',
    avi: 'video/x-msvideo',
    mp3: 'audio/mpeg',
    flac: 'audio/flac',
    wav: 'audio/wav',
    zip: 'application/zip',
    rar: 'application/x-rar-compressed',
    '7z': 'application/x-7z-compressed',
    pdf: 'application/pdf',
    exe: 'application/x-msdownload',
    iso: 'application/x-iso9660-image',
    jpg: 'image/jpeg',
    jpeg: 'image/jpeg',
    png: 'image/png',
    gif: 'image/gif',
  };
  return ext ? (mimeMap[ext] ?? `application/${ext}`) : 'application/octet-stream';
}

export function FileInfoSection({ download }: FileInfoSectionProps) {
  const mimeType = getMimeType(download.fileName);

  return (
    <section className="space-y-3">
      <h3 className="text-sm font-semibold">File Info</h3>
      <div className="space-y-2 text-xs">
        <div>
          <p className="text-muted-foreground">Name</p>
          <Tooltip>
            <TooltipTrigger asChild>
              <p className="font-mono truncate max-w-full cursor-default">{download.fileName}</p>
            </TooltipTrigger>
            <TooltipContent>
              <p className="max-w-[400px] break-all">{download.fileName}</p>
            </TooltipContent>
          </Tooltip>
        </div>
        <div>
          <p className="text-muted-foreground">Size</p>
          <p className="font-mono">{formatBytes(download.totalBytes)}</p>
        </div>
        <div>
          <p className="text-muted-foreground">MIME Type</p>
          <p className="font-mono">{mimeType}</p>
        </div>
        <div>
          <p className="text-muted-foreground">Destination</p>
          <Tooltip>
            <TooltipTrigger asChild>
              <p className="font-mono truncate max-w-full cursor-default">{download.destinationPath}</p>
            </TooltipTrigger>
            <TooltipContent>
              <p className="max-w-[400px] break-all">{download.destinationPath}</p>
            </TooltipContent>
          </Tooltip>
        </div>
      </div>
    </section>
  );
}
