import { useCallback } from 'react';
import { MoreHorizontal, Copy, Trash2, FolderOpen, RotateCcw } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { formatBytes, formatDate, formatEta, formatSpeed } from '@/lib/format';
import { useLanguage } from '@/hooks/useLanguage';
import type { HistoryView } from '@/types/download';
import { StatusBadge } from './StatusBadge';
import { deriveHistoryStatus, deriveHostname } from './filterEntries';

export interface HistoryRowActions {
  redownload: (entry: HistoryView) => void;
  copyUrl: (entry: HistoryView) => void;
  deleteEntry: (entry: HistoryView) => void;
  openFolder: (entry: HistoryView) => void;
}

interface HistoryRowProps {
  entry: HistoryView;
  actions: HistoryRowActions;
}

export function HistoryRow({ entry, actions }: HistoryRowProps) {
  const { t } = useTranslation();
  const { current: language } = useLanguage();

  const handleRedownload = useCallback(() => actions.redownload(entry), [actions, entry]);
  const handleCopy = useCallback(() => actions.copyUrl(entry), [actions, entry]);
  const handleDelete = useCallback(() => actions.deleteEntry(entry), [actions, entry]);
  const handleOpenFolder = useCallback(() => actions.openFolder(entry), [actions, entry]);

  const status = deriveHistoryStatus(entry);

  return (
    <tr
      data-testid="history-row"
      data-entry-id={entry.entryId}
      className="border-b transition-colors hover:bg-muted/50"
    >
      <td className="px-3 py-2">
        <span className="block max-w-[280px] truncate" title={entry.fileName}>
          {entry.fileName}
        </span>
      </td>
      <td className="px-3 py-2 text-xs text-muted-foreground">
        {deriveHostname(entry.url)}
      </td>
      <td className="px-3 py-2 text-xs">{formatBytes(entry.totalBytes)}</td>
      <td className="px-3 py-2 text-xs">{formatEta(entry.durationSeconds)}</td>
      <td className="px-3 py-2 text-xs">
        {formatDate(entry.completedAt * 1000, language)}
      </td>
      <td className="px-3 py-2">
        <StatusBadge status={status} />
      </td>
      <td className="px-3 py-2 text-xs">{formatSpeed(entry.avgSpeed)}</td>
      <td className="px-3 py-2 text-xs text-muted-foreground">—</td>
      <td className="px-3 py-2 text-xs text-muted-foreground">—</td>
      <td className="px-3 py-2">
        <div className="flex items-center justify-end gap-1">
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            aria-label={t('history.row.redownload')}
            onClick={handleRedownload}
          >
            <RotateCcw className="size-3.5" />
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                aria-label={t('history.row.moreActions')}
              >
                <MoreHorizontal className="size-3.5" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={handleCopy}>
                <Copy className="size-3.5" />
                {t('history.row.copyUrl')}
              </DropdownMenuItem>
              <DropdownMenuItem onClick={handleOpenFolder}>
                <FolderOpen className="size-3.5" />
                {t('history.row.openFolder')}
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem variant="destructive" onClick={handleDelete}>
                <Trash2 className="size-3.5" />
                {t('history.row.delete')}
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </td>
    </tr>
  );
}
