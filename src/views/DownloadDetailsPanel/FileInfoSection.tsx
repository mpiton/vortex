import { useTranslation } from 'react-i18next';
import { ExternalLink, FolderOpen } from 'lucide-react';
import type { DownloadDetailView } from '@/types/download';
import { formatBytes, formatDate } from '@/lib/format';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip';
import { Button } from '@/components/ui/button';
import { useTauriMutation } from '@/api/hooks';

interface FileInfoSectionProps {
  download: DownloadDetailView;
}

function getMimeType(fileName: string): string {
  const dotIndex = fileName.lastIndexOf('.');
  const ext = dotIndex > 0 && dotIndex < fileName.length - 1
    ? fileName.slice(dotIndex + 1).toLowerCase()
    : undefined;
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
  const { t, i18n } = useTranslation();
  const mimeType = getMimeType(download.fileName);

  const openFileMut = useTauriMutation<unknown, { id: number }>('download_open_file', {
    errorMessage: (err) =>
      err.message.toLowerCase().includes('not found')
        ? t('downloads.table.toast.openFileMissing')
        : t('downloads.table.toast.openFileError'),
  });
  const openFolderMut = useTauriMutation<unknown, { id: number }>('download_open_folder', {
    errorMessage: (err) =>
      err.message.toLowerCase().includes('not found')
        ? t('downloads.table.toast.openFileMissing')
        : t('downloads.table.toast.openFolderError'),
  });

  const isCompleted = download.state === 'Completed';

  return (
    <section className="space-y-3">
      <h3 className="text-sm font-semibold">{t('downloads.fileInfo')}</h3>
      <div className="space-y-2 text-xs">
        <div>
          <p className="text-muted-foreground">{t('downloads.fileName')}</p>
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
          <p className="text-muted-foreground">{t('downloads.fileSize')}</p>
          <p className="font-mono">{formatBytes(download.totalBytes)}</p>
        </div>
        <div>
          <p className="text-muted-foreground">{t('downloads.mimeType')}</p>
          <p className="font-mono">{mimeType}</p>
        </div>
        <div>
          <p className="text-muted-foreground">{t('downloads.added')}</p>
          <p className="font-mono">{formatDate(download.createdAt, i18n.language)}</p>
        </div>
        <div>
          <p className="text-muted-foreground">{t('downloads.destination')}</p>
          <Tooltip>
            <TooltipTrigger asChild>
              <p className="font-mono truncate max-w-full cursor-default">{download.destinationPath}</p>
            </TooltipTrigger>
            <TooltipContent>
              <p className="max-w-[400px] break-all">{download.destinationPath}</p>
            </TooltipContent>
          </Tooltip>
        </div>
        {isCompleted && (
          <div className="flex gap-2 pt-1">
            <Button
              variant="outline"
              size="sm"
              className="gap-1.5"
              onClick={() => openFileMut.mutate({ id: Number(download.id) })}
            >
              <ExternalLink className="size-3.5" />
              {t('downloads.table.actions.openFile')}
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="gap-1.5"
              onClick={() => openFolderMut.mutate({ id: Number(download.id) })}
            >
              <FolderOpen className="size-3.5" />
              {t('downloads.table.actions.openFolder')}
            </Button>
          </div>
        )}
      </div>
    </section>
  );
}
