import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useTauriMutation } from "@/api/hooks";
import { historyQueries } from "@/api/queries";
import { useDebouncedValue } from "@/hooks/useDebouncedValue";
import { useHistoryQuery } from "@/hooks/useHistoryQuery";
import { useRedownload } from "@/hooks/useRedownload";
import { toast } from "@/lib/toast";
import type { HistoryView as HistoryEntry } from "@/types/download";
import { deriveHistoryStatus, filterHistoryEntries, type HistoryFilterType } from "./filterEntries";
import { groupByDay } from "./groupByDay";
import { HistoryHeader } from "./HistoryHeader";
import { HistoryDayGroup } from "./HistoryDayGroup";
import type { HistoryRowActions } from "./HistoryRow";

const SEARCH_DEBOUNCE_MS = 300;
const INVALIDATE_KEYS = [historyQueries.lists()] as const;

export function HistoryView() {
  const { t } = useTranslation();
  const [searchInput, setSearchInput] = useState("");
  const [filter, setFilter] = useState<HistoryFilterType>("all");
  const debouncedSearch = useDebouncedValue(searchInput, SEARCH_DEBOUNCE_MS);

  const { data, isLoading, error } = useHistoryQuery({
    searchQuery: debouncedSearch,
  });
  const entries = useMemo<HistoryEntry[]>(() => data ?? [], [data]);

  const counts = useMemo<Record<HistoryFilterType, number>>(() => {
    const result: Record<HistoryFilterType, number> = {
      all: entries.length,
      completed: 0,
      failed: 0,
      cancelled: 0,
    };
    for (const entry of entries) {
      result[deriveHistoryStatus(entry)] += 1;
    }
    return result;
  }, [entries]);

  const filteredEntries = useMemo(
    () => filterHistoryEntries(entries, { filter, searchQuery: "" }),
    [entries, filter],
  );

  const groups = useMemo(() => groupByDay(filteredEntries), [filteredEntries]);

  const redownload = useRedownload({
    successToastKey: "history.toast.redownloadSuccess",
    errorToastKey: "history.toast.redownloadError",
  });

  const deleteMut = useTauriMutation<void, { id: string }>("history_delete_entry", {
    invalidateKeys: INVALIDATE_KEYS,
    errorMessage: () => t("history.toast.deleteError"),
  });

  const exportMut = useTauriMutation<number, { format: "csv" | "json"; path: string }>(
    "history_export",
    {
      errorMessage: () => t("history.toast.exportError"),
    },
  );

  const openFolderMut = useTauriMutation<void, { path: string }>("reveal_in_folder", {
    errorMessage: () => t("history.toast.openFolderError"),
  });

  const handleRedownload = useCallback(
    (entry: HistoryEntry) => {
      redownload.trigger("history", entry.entryId);
    },
    [redownload],
  );

  const handleCopyUrl = useCallback(
    (entry: HistoryEntry) => {
      void navigator.clipboard.writeText(entry.url).then(
        () => toast.success(t("history.toast.copySuccess")),
        () => toast.error(t("history.toast.copyError")),
      );
    },
    [t],
  );

  const handleDelete = useCallback(
    (entry: HistoryEntry) => {
      deleteMut.mutate(
        { id: entry.entryId },
        { onSuccess: () => toast.success(t("history.toast.deleteSuccess")) },
      );
    },
    [deleteMut, t],
  );

  const handleOpenFolder = useCallback(
    (entry: HistoryEntry) => {
      openFolderMut.mutate({ path: entry.destinationPath });
    },
    [openFolderMut],
  );

  const rowActions = useMemo<HistoryRowActions>(
    () => ({
      redownload: handleRedownload,
      copyUrl: handleCopyUrl,
      deleteEntry: handleDelete,
      openFolder: handleOpenFolder,
    }),
    [handleRedownload, handleCopyUrl, handleDelete, handleOpenFolder],
  );

  const handleExport = useCallback(
    async (format: "csv" | "json") => {
      const extension = format === "csv" ? "csv" : "json";
      let selected: string | null;
      try {
        selected = await saveDialog({
          defaultPath: `vortex-history-${Date.now()}.${extension}`,
          filters: [{ name: format.toUpperCase(), extensions: [extension] }],
        });
      } catch {
        toast.error(t("history.toast.exportError"));
        return;
      }
      if (!selected) return;
      exportMut.mutate(
        { format, path: selected },
        {
          onSuccess: (count) =>
            toast.success(
              t("history.toast.exportSuccess", { count, format: format.toUpperCase() }),
            ),
        },
      );
    },
    [exportMut, t],
  );

  const hasActiveSearch = debouncedSearch.trim().length > 0;
  const isSearchEmpty = !isLoading && hasActiveSearch && entries.length === 0;
  const isEmpty = !isLoading && !hasActiveSearch && entries.length === 0;
  const isFilterEmpty = !isLoading && entries.length > 0 && filteredEntries.length === 0;

  return (
    <div className="flex h-full min-h-0 flex-col gap-3 p-4">
      {redownload.dialog}
      <HistoryHeader
        search={searchInput}
        onSearchChange={setSearchInput}
        filter={filter}
        onFilterChange={setFilter}
        counts={counts}
        onExport={handleExport}
        exportDisabled={entries.length === 0}
      />
      <div className="flex-1 overflow-auto rounded-md border">
        {error && (
          <div className="flex h-full items-center justify-center text-sm text-destructive">
            {error.message}
          </div>
        )}
        {!error && isLoading && (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            {t("history.loading")}
          </div>
        )}
        {!error && isEmpty && (
          <div
            data-testid="history-empty"
            className="flex h-full items-center justify-center text-sm text-muted-foreground"
          >
            {t("history.empty")}
          </div>
        )}
        {!error && isSearchEmpty && (
          <div
            data-testid="history-search-empty"
            className="flex h-full items-center justify-center text-sm text-muted-foreground"
          >
            {t("history.searchEmpty", { query: debouncedSearch.trim() })}
          </div>
        )}
        {!error && isFilterEmpty && (
          <div
            data-testid="history-filter-empty"
            className="flex h-full items-center justify-center text-sm text-muted-foreground"
          >
            {t("history.filterEmpty")}
          </div>
        )}
        {!error && !isLoading && groups.length > 0 && (
          <div className="flex flex-col">
            {groups.map((group) => (
              <HistoryDayGroup key={group.dayKey} group={group} actions={rowActions} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
