import { useRef, useMemo, useState, useCallback, createContext, useContext } from "react";
import {
  useReactTable,
  getCoreRowModel,
  getSortedRowModel,
  flexRender,
} from "@tanstack/react-table";
import type { ColumnDef, SortingState } from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import { DndContext, PointerSensor, useSensor, useSensors, closestCenter } from "@dnd-kit/core";
import type { DragEndEvent } from "@dnd-kit/core";
import {
  SortableContext,
  verticalListSortingStrategy,
  useSortable,
  arrayMove,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { useTranslation } from "react-i18next";
import {
  Pause,
  Play,
  RotateCcw,
  MoreHorizontal,
  Trash2,
  ArrowUp,
  ArrowDown,
  ArrowUpToLine,
  ArrowDownToLine,
  ExternalLink,
  FolderOpen,
  RefreshCw,
  GripVertical,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useTauriMutation } from "@/api/hooks";
import { downloadQueries } from "@/api/queries";
import { useUiStore } from "@/stores/uiStore";
import { useRedownload } from "@/hooks/useRedownload";
import type { DownloadView, DownloadState } from "@/types/download";
import type { FilterType } from "./types";
import { StateIndicator } from "./StateIndicator";
import { ProgressCell } from "./ProgressCell";
import { SpeedCell } from "./SpeedCell";
import { EtaCell } from "./EtaCell";
import { WaitCountdownCell } from "./WaitCountdownCell";

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
  active: ["Downloading", "Queued"],
  queued: ["Queued"],
  done: ["Completed"],
  failed: ["Error", "Retry"],
};

function extractExtension(fileName: string): string {
  const dot = fileName.lastIndexOf(".");
  if (dot <= 0 || dot === fileName.length - 1) return "";
  return fileName.slice(dot + 1).toUpperCase();
}

function extractHostname(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return "\u2014";
  }
}

export function filterDownloads(
  downloads: DownloadView[],
  { downloadsAreFiltered = false, filter = "all", searchQuery = "" }: FilterDownloadsOptions = {},
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
  moveToTop: (id: string) => void;
  moveToBottom: (id: string) => void;
  openFile: (id: string) => void;
  openFolder: (id: string) => void;
  redownload: (id: string) => void;
}

const REORDERABLE_STATES: DownloadState[] = ["Queued", "Retry", "Waiting"];

export function isReorderable(state: DownloadState): boolean {
  return REORDERABLE_STATES.includes(state);
}

/// Returns the new ordered list of reorderable IDs after a drag-and-drop move.
/// Pure helper extracted so drag handler logic is exercisable in isolation.
export function computeReorderedIds(
  downloads: DownloadView[],
  activeId: string,
  overId: string,
): number[] | null {
  if (activeId === overId) return null;
  const ids = downloads.map((d) => d.id);
  const oldIndex = ids.indexOf(activeId);
  const newIndex = ids.indexOf(overId);
  if (oldIndex < 0 || newIndex < 0) return null;
  const nextOrder = arrayMove(ids, oldIndex, newIndex);
  const byId = new Map(downloads.map((d) => [d.id, d]));
  return nextOrder
    .filter((id) => {
      const dl = byId.get(id);
      return dl ? isReorderable(dl.state) : false;
    })
    .map((id) => Number(id));
}

const RowActionsContext = createContext<RowActions | null>(null);

function useRowActions(): RowActions {
  const ctx = useContext(RowActionsContext);
  if (!ctx) throw new Error("useRowActions must be inside RowActionsContext");
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
      {download.state === "Downloading" && (
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          aria-label={t("downloads.table.actions.pause")}
          onClick={(e) => {
            e.stopPropagation();
            actions.pause(download.id);
          }}
        >
          <Pause className="size-3.5" />
        </Button>
      )}
      {download.state === "Paused" && (
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          aria-label={t("downloads.table.actions.resume")}
          onClick={(e) => {
            e.stopPropagation();
            actions.resume(download.id);
          }}
        >
          <Play className="size-3.5" />
        </Button>
      )}
      {download.state === "Error" && (
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          aria-label={t("downloads.table.actions.retry")}
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
            aria-label={t("downloads.table.moreActions")}
            onClick={(e) => e.stopPropagation()}
          >
            <MoreHorizontal className="size-3.5" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          {download.state === "Completed" && (
            <>
              <DropdownMenuItem
                onClick={(e) => {
                  e.stopPropagation();
                  actions.openFile(download.id);
                }}
              >
                <ExternalLink className="size-3.5" />
                {t("downloads.table.actions.openFile")}
              </DropdownMenuItem>
              <DropdownMenuItem
                onClick={(e) => {
                  e.stopPropagation();
                  actions.openFolder(download.id);
                }}
              >
                <FolderOpen className="size-3.5" />
                {t("downloads.table.actions.openFolder")}
              </DropdownMenuItem>
              <DropdownMenuItem
                onClick={(e) => {
                  e.stopPropagation();
                  actions.redownload(download.id);
                }}
              >
                <RefreshCw className="size-3.5" />
                {t("downloads.table.actions.redownload")}
              </DropdownMenuItem>
              <DropdownMenuSeparator />
            </>
          )}
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>
              {t("downloads.table.actions.setPriority")}
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent>
              {[
                { label: t("downloads.table.priority.high"), value: 1 },
                { label: t("downloads.table.priority.normal"), value: 5 },
                { label: t("downloads.table.priority.low"), value: 10 },
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
          {isReorderable(download.state) && (
            <>
              <DropdownMenuItem
                onClick={(e) => {
                  e.stopPropagation();
                  actions.moveToTop(download.id);
                }}
              >
                <ArrowUpToLine className="size-3.5" />
                {t("downloads.table.actions.moveToTop")}
              </DropdownMenuItem>
              <DropdownMenuItem
                onClick={(e) => {
                  e.stopPropagation();
                  actions.moveToBottom(download.id);
                }}
              >
                <ArrowDownToLine className="size-3.5" />
                {t("downloads.table.actions.moveToBottom")}
              </DropdownMenuItem>
            </>
          )}
          <DropdownMenuSeparator />
          <DropdownMenuItem
            variant="destructive"
            onClick={(e) => {
              e.stopPropagation();
              actions.remove(download.id);
            }}
          >
            <Trash2 className="size-3.5" />
            {t("downloads.table.actions.remove")}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

type DragHandleProps = {
  listeners: ReturnType<typeof useSortable>["listeners"];
  attributes: ReturnType<typeof useSortable>["attributes"];
  setActivatorNodeRef: ReturnType<typeof useSortable>["setActivatorNodeRef"];
  enabled: boolean;
};

const DragHandleContext = createContext<DragHandleProps | null>(null);

function DragHandleCell() {
  const ctx = useContext(DragHandleContext);
  if (!ctx || !ctx.enabled) {
    return <div className="w-4" aria-hidden="true" />;
  }
  return (
    <button
      type="button"
      aria-label="Drag to reorder"
      className="flex size-4 cursor-grab items-center justify-center text-muted-foreground hover:text-foreground"
      ref={ctx.setActivatorNodeRef}
      {...ctx.attributes}
      {...ctx.listeners}
      onClick={(e) => e.stopPropagation()}
    >
      <GripVertical className="size-3.5" />
    </button>
  );
}

interface SortableRowProps {
  id: string;
  state: DownloadState;
  children: React.ReactNode;
  onClick?: (e: React.MouseEvent) => void;
  className?: string;
  dataIndex: number;
  measureRef: (node: HTMLElement | null) => void;
}

function SortableRow({
  id,
  state,
  children,
  onClick,
  className,
  dataIndex,
  measureRef,
}: SortableRowProps) {
  const enabled = isReorderable(state);
  const sortable = useSortable({ id, disabled: !enabled });

  const { setNodeRef } = sortable;
  const setRef = useCallback(
    (node: HTMLTableRowElement | null) => {
      setNodeRef(node);
      measureRef(node);
    },
    [setNodeRef, measureRef],
  );

  const style: React.CSSProperties = {
    transform: CSS.Transform.toString(sortable.transform),
    transition: sortable.transition,
    opacity: sortable.isDragging ? 0.6 : undefined,
  };

  const ctxValue = useMemo<DragHandleProps>(
    () => ({
      listeners: sortable.listeners,
      attributes: sortable.attributes,
      setActivatorNodeRef: sortable.setActivatorNodeRef,
      enabled,
    }),
    [sortable.listeners, sortable.attributes, sortable.setActivatorNodeRef, enabled],
  );

  return (
    <DragHandleContext value={ctxValue}>
      <tr ref={setRef} data-index={dataIndex} style={style} className={className} onClick={onClick}>
        {children}
      </tr>
    </DragHandleContext>
  );
}

function getColumns(t: Translate): ColumnDef<DownloadView>[] {
  return [
    {
      id: "drag",
      header: "",
      cell: () => <DragHandleCell />,
      enableSorting: false,
    },
    {
      id: "select",
      header: ({ table }) => (
        <Checkbox
          checked={table.getIsAllRowsSelected()}
          onCheckedChange={(checked) => table.toggleAllRowsSelected(!!checked)}
          aria-label={t("downloads.table.selectAll")}
        />
      ),
      cell: ({ row }) => (
        <Checkbox
          checked={row.getIsSelected()}
          onCheckedChange={(checked) => row.toggleSelected(!!checked)}
          onClick={(e) => e.stopPropagation()}
          aria-label={t("downloads.table.selectRow")}
        />
      ),
      enableSorting: false,
    },
    {
      accessorKey: "state",
      header: t("downloads.table.columns.state"),
      cell: ({ row }) => (
        <StateIndicator state={row.original.state} errorMessage={row.original.errorMessage} />
      ),
    },
    {
      accessorKey: "fileName",
      header: t("downloads.table.columns.fileName"),
      cell: ({ row }) => (
        <Tooltip>
          <TooltipTrigger asChild>
            <span className="block max-w-[200px] truncate">{row.original.fileName}</span>
          </TooltipTrigger>
          <TooltipContent>
            <p className="max-w-[400px] break-all">{row.original.url}</p>
          </TooltipContent>
        </Tooltip>
      ),
    },
    {
      id: "type",
      header: t("downloads.table.columns.type"),
      cell: ({ row }) => {
        const ext = extractExtension(row.original.fileName);
        return ext ? <Badge variant="outline">{ext}</Badge> : null;
      },
    },
    {
      id: "host",
      header: t("downloads.table.columns.host"),
      cell: ({ row }) => (
        <span className="text-xs text-muted-foreground">
          {row.original.sourceHostname || extractHostname(row.original.url)}
        </span>
      ),
    },
    {
      accessorKey: "progressPercent",
      header: t("downloads.table.columns.progress"),
      cell: ({ row }) => <ProgressCell download={row.original} />,
    },
    {
      id: "speed",
      header: t("downloads.table.columns.speed"),
      cell: ({ row }) => <SpeedCell downloadId={row.original.id} />,
      enableSorting: false,
    },
    {
      id: "eta",
      header: t("downloads.table.columns.eta"),
      cell: ({ row }) =>
        row.original.state === "Waiting" ? (
          <WaitCountdownCell downloadId={row.original.id} />
        ) : (
          <EtaCell downloadId={row.original.id} />
        ),
      enableSorting: false,
    },
    {
      id: "actions",
      header: "",
      cell: ({ row }) => <ActionCell download={row.original} t={t} />,
      enableSorting: false,
    },
  ];
}

export function DownloadsTable({
  downloads,
  downloadsAreFiltered = false,
  isLoading,
  filter = "all",
  searchQuery = "",
}: DownloadsTableProps) {
  const { t } = useTranslation();
  const [sorting, setSorting] = useState<SortingState>([]);
  const tableContainerRef = useRef<HTMLDivElement>(null);
  const columns = useMemo(() => getColumns(t), [t]);

  const invalidateKeys = useMemo(
    () => [downloadQueries.lists(), downloadQueries.countByState()] as const,
    [],
  );
  const pauseMut = useTauriMutation<unknown, { id: number }>("download_pause", { invalidateKeys });
  const resumeMut = useTauriMutation<unknown, { id: number }>("download_resume", {
    invalidateKeys,
  });
  const retryMut = useTauriMutation<unknown, { id: number }>("download_retry", { invalidateKeys });
  const removeMut = useTauriMutation<unknown, { id: number; deleteFiles: boolean }>(
    "download_remove",
    { invalidateKeys },
  );
  const priorityMut = useTauriMutation<unknown, { id: number; priority: number }>(
    "download_set_priority",
    { invalidateKeys },
  );
  const moveToTopMut = useTauriMutation<unknown, { id: number }>("download_move_to_top", {
    invalidateKeys,
  });
  const moveToBottomMut = useTauriMutation<unknown, { id: number }>("download_move_to_bottom", {
    invalidateKeys,
  });
  const reorderMut = useTauriMutation<unknown, { orderedIds: number[] }>("download_reorder_queue", {
    invalidateKeys,
  });
  const openFileMut = useTauriMutation<unknown, { id: number }>("download_open_file", {
    errorMessage: (err) =>
      err.message.toLowerCase().includes("not found")
        ? t("downloads.table.toast.openFileMissing")
        : t("downloads.table.toast.openFileError"),
  });
  const openFolderMut = useTauriMutation<unknown, { id: number }>("download_open_folder", {
    errorMessage: (err) =>
      err.message.toLowerCase().includes("not found")
        ? t("downloads.table.toast.openFileMissing")
        : t("downloads.table.toast.openFolderError"),
  });
  const redownload = useRedownload();

  const rowActions = useMemo<RowActions>(
    () => ({
      pause: (id) => pauseMut.mutate({ id: Number(id) }),
      resume: (id) => resumeMut.mutate({ id: Number(id) }),
      start: (id) => retryMut.mutate({ id: Number(id) }),
      remove: (id) => removeMut.mutate({ id: Number(id), deleteFiles: false }),
      setPriority: (id, priority) => priorityMut.mutate({ id: Number(id), priority }),
      moveToTop: (id) => moveToTopMut.mutate({ id: Number(id) }),
      moveToBottom: (id) => moveToBottomMut.mutate({ id: Number(id) }),
      openFile: (id) => openFileMut.mutate({ id: Number(id) }),
      openFolder: (id) => openFolderMut.mutate({ id: Number(id) }),
      redownload: (id) => redownload.trigger("download", id),
    }),
    [
      pauseMut,
      resumeMut,
      retryMut,
      removeMut,
      priorityMut,
      moveToTopMut,
      moveToBottomMut,
      openFileMut,
      openFolderMut,
      redownload,
    ],
  );

  const selectedDownloadIds = useUiStore((s) => s.selectedDownloadIds);
  const selectDownload = useUiStore((s) => s.selectDownload);
  const setSelectedDownloadIds = useUiStore((s) => s.setSelectedDownloadIds);
  const toggleDownloadSelection = useUiStore((s) => s.toggleDownloadSelection);
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
      const next = typeof updater === "function" ? updater(rowSelection) : updater;
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

  // Build the dnd-kit sortable id list from the visually sorted rows so
  // collision/strategy calculations match what the user sees when a column
  // sort is active.
  const sortableIds = useMemo(() => rows.map((r) => r.original.id), [rows]);

  const sensors = useSensors(useSensor(PointerSensor, { activationConstraint: { distance: 4 } }));

  const sortedDownloads = useMemo(() => rows.map((r) => r.original), [rows]);

  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over) return;
      const orderedIds = computeReorderedIds(sortedDownloads, String(active.id), String(over.id));
      if (!orderedIds || orderedIds.length === 0) return;
      reorderMut.mutate({ orderedIds });
    },
    [sortedDownloads, reorderMut],
  );

  if (isLoading) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <span className="text-sm text-muted-foreground">{t("downloads.loading")}</span>
      </div>
    );
  }

  if (filteredDownloads.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <span className="text-sm text-muted-foreground">{t("downloads.empty")}</span>
      </div>
    );
  }

  const virtualRows = rowVirtualizer.getVirtualItems();
  const totalSize = rowVirtualizer.getTotalSize();

  return (
    <RowActionsContext value={rowActions}>
      {redownload.dialog}
      <DndContext sensors={sensors} collisionDetection={closestCenter} onDragEnd={handleDragEnd}>
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
                          : flexRender(header.column.columnDef.header, header.getContext())}
                        {header.column.getIsSorted() === "asc" && <ArrowUp className="size-3" />}
                        {header.column.getIsSorted() === "desc" && <ArrowDown className="size-3" />}
                      </div>
                    </th>
                  ))}
                </tr>
              ))}
            </thead>
            <tbody>
              {virtualRows.length > 0 && (
                <tr>
                  <td colSpan={columns.length} style={{ height: virtualRows[0].start }} />
                </tr>
              )}
              <SortableContext items={sortableIds} strategy={verticalListSortingStrategy}>
                {virtualRows.map((virtualRow) => {
                  const row = rows[virtualRow.index];
                  const isSelected = rowSelection[row.original.id] === true;

                  return (
                    <SortableRow
                      key={row.id}
                      id={row.original.id}
                      state={row.original.state}
                      dataIndex={virtualRow.index}
                      measureRef={(node) => rowVirtualizer.measureElement(node)}
                      className={`border-b transition-colors hover:bg-muted/50 ${
                        isSelected ? "bg-accent/50" : ""
                      }`}
                      onClick={(e) => handleRowClick(e, row.original.id)}
                    >
                      {row.getVisibleCells().map((cell) => (
                        <td key={cell.id} className="px-3 py-2">
                          {flexRender(cell.column.columnDef.cell, cell.getContext())}
                        </td>
                      ))}
                    </SortableRow>
                  );
                })}
              </SortableContext>
              {virtualRows.length > 0 && (
                <tr>
                  <td
                    colSpan={columns.length}
                    style={{
                      height: totalSize - virtualRows[virtualRows.length - 1].end,
                    }}
                  />
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </DndContext>
    </RowActionsContext>
  );
}
