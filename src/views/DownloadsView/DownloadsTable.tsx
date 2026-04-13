import { useRef, useMemo, useState, useCallback, createContext, useContext } from 'react';
import {
  useReactTable,
  getCoreRowModel,
  getSortedRowModel,
  flexRender,
} from '@tanstack/react-table';
import type { ColumnDef, SortingState } from '@tanstack/react-table';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useTranslation } from 'react-i18next';
import {
  Pause,
  Play,
  RotateCcw,
  MoreHorizontal,
  Trash2,
  ArrowUp,
  ArrowDown,
} from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useTauriMutation } from '@/api/hooks';
import { downloadQueries } from '@/api/queries';
import { useUiStore } from '@/stores/uiStore';
import type { DownloadView, DownloadState } from '@/types/download';
import type { FilterType } from './types';
import { StateIndicator } from './StateIndicator';
import { ProgressCell } from './ProgressCell';
import { SpeedCell } from './SpeedCell';
import { EtaCell } from './EtaCell';

type Translate = (key: string, options?: Record<string, unknown>) => string;

interface DownloadsTableProps {
  downloads: DownloadView[];
  downloadsAreFiltered?: boolean;
  isLoading: boolean;
  filter?: FilterType;
  searchQuery?: string;
}

interface FilterDownloadsOptions {
  downloadsAreFiltered?: boolean;
  filter?: FilterType;
  searchQuery?: string;
}

const STATE_FILTER_MAP: Record<FilterType, DownloadState[] | null> = {
  all: null,
  active: ['Downloading', 'Queued'],
  queued: ['Queued'],
  done: ['Completed'],
  failed: ['Error', 'Retry'],
};

function extractExtension(fileName: string): string {
  const dot = fileName.lastIndexOf('.');
  if (dot <= 0 || dot === fileName.length - 1) return '';
  return fileName.slice(dot + 1).toUpperCase();
}

function extractHostname(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return '\u2014';
  }
}

export function filterDownloads(
  downloads: DownloadView[],
  {
    downloadsAreFiltered = false,
    filter = 'all',
    searchQuery = '',
  }: FilterDownloadsOptions = {},
) {
  if (downloadsAreFiltered) {
    return downloads;
  }

  let result = downloads;

  const allowedStates = STATE_FILTER_MAP[filter];
  if (allowedStates) {
    result = result.filter((download) => allowedStates.includes(download.state));
  }

  if (!searchQuery) {
    return result;
  }

  const query = searchQuery.toLowerCase();
  return result.filter(
    (download) =>
      download.fileName.toLowerCase().includes(query) ||
      download.url.toLowerCase().includes(query) ||
      extractHostname(download.url).toLowerCase().includes(query),
  );
}

interface RowActions {
  pause: (id: string) => void;
  resume: (id: string) => void;
  start: (id: string) => void;
  remove: (id: string) => void;
  setPriority: (id: string, priority: number) => void;
}

const RowActionsContext = createContext<RowActions | null>(null);

function useRowActions(): RowActions {
  const ctx = useContext(RowActionsContext);
  if (!ctx) throw new Error('useRowActions must be inside RowActionsContext');
  return ctx;
}

interface ActionCellProps {
  download: DownloadView;
  t: Translate;
}

function ActionCell({ download, t }: ActionCellProps) {
  const actions = useRowActions();

  return (
    <div className="flex items-center gap-1">
      {(download.state === 'Downloading' || download.state === 'Queued') && (
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          onClick={(e) => {
            e.stopPropagation();
            actions.pause(download.id);
          }}
        >
          <Pause className="size-3.5" />
        </Button>
      )}
      {download.state === 'Paused' && (
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          onClick={(e) => {
            e.stopPropagation();
            actions.resume(download.id);
          }}
        >
          <Play className="size-3.5" />
        </Button>
      )}
      {(download.state === 'Error' || download.state === 'Retry') && (
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          onClick={(e) => {
            e.stopPropagation();
            actions.start(download.id);
          }}
        >
          <RotateCcw className="size-3.5" />
        </Button>
      )}
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            aria-label={t('downloads.table.moreActions')}
            onClick={(e) => e.stopPropagation()}
          >
            <MoreHorizontal className="size-3.5" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>{t('downloads.table.actions.setPriority')}</DropdownMenuSubTrigger>
            <DropdownMenuSubContent>
              {[
                { label: t('downloads.table.priority.high'), value: 1 },
                { label: t('downloads.table.priority.normal'), value: 5 },
                { label: t('downloads.table.priority.low'), value: 10 },
              ].map((p) => (
                <DropdownMenuItem
                  key={p.value}
                  onClick={(e) => {
                    e.stopPropagation();
                    actions.setPriority(download.id, p.value);
                  }}
                >
                  {p.label}
                </DropdownMenuItem>
              ))}
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuSeparator />
          <DropdownMenuItem
            variant="destructive"
            onClick={(e) => {
              e.stopPropagation();
              actions.remove(download.id);
            }}
          >
            <Trash2 className="size-3.5" />
            {t('downloads.table.actions.remove')}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

function getColumns(t: Translate): ColumnDef<DownloadView>[] {
  return [
    {
      id: 'select',
      header: ({ table }) => (
        <Checkbox
          checked={table.getIsAllRowsSelected()}
          onCheckedChange={(checked) => table.toggleAllRowsSelected(!!checked)}
          aria-label={t('downloads.table.selectAll')}
        />
      ),
      cell: ({ row }) => (
        <Checkbox
          checked={row.getIsSelected()}
          onCheckedChange={(checked) => row.toggleSelected(!!checked)}
          onClick={(e) => e.stopPropagation()}
          aria-label={t('downloads.table.selectRow')}
        />
      ),
      enableSorting: false,
    },
    {
      accessorKey: 'state',
      header: t('downloads.table.columns.state'),
      cell: ({ row }) => <StateIndicator state={row.original.state} />,
    },
    {
      accessorKey: 'fileName',
      header: t('downloads.table.columns.fileName'),
      cell: ({ row }) => (
        <Tooltip>
          <TooltipTrigger asChild>
            <span className="block max-w-[200px] truncate">
              {row.original.fileName}
            </span>
          </TooltipTrigger>
          <TooltipContent>
            <p className="max-w-[400px] break-all">{row.original.url}</p>
          </TooltipContent>
        </Tooltip>
      ),
    },
    {
      id: 'type',
      header: t('downloads.table.columns.type'),
      cell: ({ row }) => {
        const ext = extractExtension(row.original.fileName);
        return ext ? <Badge variant="outline">{ext}</Badge> : null;
      },
    },
    {
      id: 'host',
      header: t('downloads.table.columns.host'),
      cell: ({ row }) => (
        <span className="text-xs text-muted-foreground">
          {extractHostname(row.original.url)}
        </span>
      ),
    },
    {
      accessorKey: 'progressPercent',
      header: t('downloads.table.columns.progress'),
      cell: ({ row }) => <ProgressCell download={row.original} />,
    },
    {
      id: 'speed',
      header: t('downloads.table.columns.speed'),
      cell: ({ row }) => <SpeedCell downloadId={row.original.id} />,
      enableSorting: false,
    },
    {
      id: 'eta',
      header: t('downloads.table.columns.eta'),
      cell: ({ row }) => <EtaCell downloadId={row.original.id} />,
      enableSorting: false,
    },
    {
      id: 'actions',
      header: '',
      cell: ({ row }) => <ActionCell download={row.original} t={t} />,
      enableSorting: false,
    },
  ];
}

export function DownloadsTable({
  downloads,
  downloadsAreFiltered = false,
  isLoading,
  filter = 'all',
  searchQuery = '',
}: DownloadsTableProps) {
  const { t } = useTranslation();
  const [sorting, setSorting] = useState<SortingState>([]);
  const tableContainerRef = useRef<HTMLDivElement>(null);
  const columns = useMemo(() => getColumns(t), [t]);

  const invalidateKeys = useMemo(() => [downloadQueries.lists(), downloadQueries.countByState()] as const, []);
  const pauseMut = useTauriMutation<unknown, { id: string }>('download_pause', { invalidateKeys });
  const resumeMut = useTauriMutation<unknown, { id: string }>('download_resume', { invalidateKeys });
  const startMut = useTauriMutation<unknown, { id: string }>('download_start', { invalidateKeys });
  const removeMut = useTauriMutation<unknown, { id: string; deleteFiles: boolean }>('download_remove', { invalidateKeys });
  const priorityMut = useTauriMutation<unknown, { id: string; priority: number }>('download_set_priority', { invalidateKeys });

  const rowActions = useMemo<RowActions>(() => ({
    pause: (id) => pauseMut.mutate({ id }),
    resume: (id) => resumeMut.mutate({ id }),
    start: (id) => startMut.mutate({ id }),
    remove: (id) => removeMut.mutate({ id, deleteFiles: false }),
    setPriority: (id, priority) => priorityMut.mutate({ id, priority }),
  }), [pauseMut, resumeMut, startMut, removeMut, priorityMut]);

  const selectedDownloadIds = useUiStore((s) => s.selectedDownloadIds);
  const selectDownload = useUiStore((s) => s.selectDownload);
  const setSelectedDownloadIds = useUiStore((s) => s.setSelectedDownloadIds);
  const toggleDownloadSelection = useUiStore(
    (s) => s.toggleDownloadSelection,
  );
  const clearSelection = useUiStore((s) => s.clearSelection);

  const filteredDownloads = useMemo(
    () =>
      filterDownloads(downloads, {
        downloadsAreFiltered,
        filter,
        searchQuery,
      }),
    [downloads, downloadsAreFiltered, filter, searchQuery],
  );

  const rowSelection = useMemo(() => {
    const selectedSet = new Set(selectedDownloadIds);
    const map: Record<string, boolean> = {};
    for (const dl of filteredDownloads) {
      if (selectedSet.has(dl.id)) {
        map[dl.id] = true;
      }
    }
    return map;
  }, [filteredDownloads, selectedDownloadIds]);

  const table = useReactTable({
    data: filteredDownloads,
    columns,
    state: { sorting, rowSelection },
    getRowId: (row) => row.id,
    onSortingChange: setSorting,
    onRowSelectionChange: (updater) => {
      const next =
        typeof updater === 'function' ? updater(rowSelection) : updater;
      setSelectedDownloadIds(Object.keys(next).filter((k) => next[k]));
    },
    getCoreRowModel: getCoreRowModel(),
    getSortedRowModel: getSortedRowModel(),
    enableRowSelection: true,
  });

  const { rows } = table.getRowModel();

  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => tableContainerRef.current,
    estimateSize: () => 48,
    overscan: 10,
  });

  const handleRowClick = useCallback(
    (e: React.MouseEvent, downloadId: string) => {
      if (e.ctrlKey || e.metaKey) {
        toggleDownloadSelection(downloadId);
      } else {
        clearSelection();
        selectDownload(downloadId);
        setSelectedDownloadIds([downloadId]);
      }
    },
    [toggleDownloadSelection, clearSelection, selectDownload, setSelectedDownloadIds],
  );

  if (isLoading) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <span className="text-sm text-muted-foreground">{t('downloads.loading')}</span>
      </div>
    );
  }

  if (filteredDownloads.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <span className="text-sm text-muted-foreground">{t('downloads.empty')}</span>
      </div>
    );
  }

  const virtualRows = rowVirtualizer.getVirtualItems();
  const totalSize = rowVirtualizer.getTotalSize();

  return (
    <RowActionsContext value={rowActions}>
    <div ref={tableContainerRef} className="flex-1 overflow-auto rounded-md border">
      <table className="w-full text-sm">
        <thead className="sticky top-0 z-10 border-b bg-background">
          {table.getHeaderGroups().map((headerGroup) => (
            <tr key={headerGroup.id}>
              {headerGroup.headers.map((header) => (
                <th
                  key={header.id}
                  className="cursor-pointer select-none px-3 py-2 text-left text-xs font-medium text-muted-foreground hover:text-foreground"
                  onClick={
                    header.column.getCanSort()
                      ? header.column.getToggleSortingHandler()
                      : undefined
                  }
                >
                  <div className="flex items-center gap-1">
                    {header.isPlaceholder
                      ? null
                      : flexRender(
                          header.column.columnDef.header,
                          header.getContext(),
                        )}
                    {header.column.getIsSorted() === 'asc' && (
                      <ArrowUp className="size-3" />
                    )}
                    {header.column.getIsSorted() === 'desc' && (
                      <ArrowDown className="size-3" />
                    )}
                  </div>
                </th>
              ))}
            </tr>
          ))}
        </thead>
        <tbody>
          {virtualRows.length > 0 && (
            <tr>
              <td
                colSpan={columns.length}
                style={{ height: virtualRows[0].start }}
              />
            </tr>
          )}
          {virtualRows.map((virtualRow) => {
            const row = rows[virtualRow.index];
            const isSelected = rowSelection[row.original.id] === true;

            return (
              <tr
                key={row.id}
                data-index={virtualRow.index}
                ref={(node) => rowVirtualizer.measureElement(node)}
                className={`border-b transition-colors hover:bg-muted/50 ${
                  isSelected ? 'bg-accent/50' : ''
                }`}
                onClick={(e) => handleRowClick(e, row.original.id)}
              >
                {row.getVisibleCells().map((cell) => (
                  <td key={cell.id} className="px-3 py-2">
                    {flexRender(cell.column.columnDef.cell, cell.getContext())}
                  </td>
                ))}
              </tr>
            );
          })}
          {virtualRows.length > 0 && (
            <tr>
              <td
                colSpan={columns.length}
                style={{
                  height:
                    totalSize -
                    virtualRows[virtualRows.length - 1].end,
                }}
              />
            </tr>
          )}
        </tbody>
      </table>
    </div>
    </RowActionsContext>
  );
}
