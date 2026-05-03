import { useRef, useState } from "react";
import { CheckCheck, FolderInput, Pause, Play, X, XCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { MoveDialog } from "@/components/ui/MoveDialog";
import { useTauriMutation, useTauriQuery } from "@/api/hooks";
import { downloadQueries } from "@/api/queries";
import { useDownloadDetail } from "@/hooks/useDownloadDetail";
import { useUiStore } from "@/stores/uiStore";
import { toast } from "@/lib/toast";
import { ClearDownloadsDialog, type ClearDownloadsTarget } from "./ClearDownloadsDialog";

interface ChangeDirectoryFailure {
  id: number;
  message: string;
}

interface ChangeDirectoryBulkOutcome {
  moved: number[];
  failed: ChangeDirectoryFailure[];
}

const INVALIDATE_KEYS = [downloadQueries.lists(), downloadQueries.countByState()] as const;

export function ActionsBar() {
  const { t } = useTranslation();
  const selectedDownloadIds = useUiStore((s) => s.selectedDownloadIds);
  const setSelectedDownloadIds = useUiStore((s) => s.setSelectedDownloadIds);
  const clearSelection = useUiStore((s) => s.clearSelection);
  const narrowSelectionToFailed = (failedIds: string[]) => {
    // Update both selection fields together so the details panel never ends
    // up focused on a row that is no longer in the multi-select set.
    useUiStore.setState((state) => ({
      selectedDownloadIds: failedIds,
      selectedDownloadId:
        state.selectedDownloadId !== null && failedIds.includes(state.selectedDownloadId)
          ? state.selectedDownloadId
          : (failedIds[0] ?? null),
    }));
  };

  const pauseAll = useTauriMutation<void, void>("download_pause_all", {
    invalidateKeys: INVALIDATE_KEYS,
  });

  const resumeAll = useTauriMutation<void, void>("download_resume_all", {
    invalidateKeys: INVALIDATE_KEYS,
  });

  const cancelDownload = useTauriMutation<void, { id: number }>("download_cancel", {
    invalidateKeys: INVALIDATE_KEYS,
  });

  const moveDownloads = useTauriMutation<
    ChangeDirectoryBulkOutcome,
    { ids: number[]; newDestinationDir: string }
  >("download_change_directory_bulk", {
    invalidateKeys: INVALIDATE_KEYS,
    onSuccess: (outcome, vars) => {
      const total = vars.ids.length;
      if (outcome.failed.length === 0) {
        toast.success(t("downloads.toast.moveSucceeded", { count: outcome.moved.length }));
        clearSelection();
      } else {
        toast.error(
          t("downloads.toast.movePartial", {
            moved: outcome.moved.length,
            total,
            failed: outcome.failed.length,
          }),
        );
        // Keep failed rows selected so the user can retry against another
        // folder without re-picking each download. The store holds ids as
        // strings; the IPC outcome surfaces them as numbers, so coerce back.
        // Also re-anchor the focused row so the details pane doesn't show a
        // download that just moved successfully.
        narrowSelectionToFailed(outcome.failed.map((f) => String(f.id)));
      }
    },
    onError: (err) => {
      toast.error(t("downloads.toast.moveError", { error: err.message }));
    },
  });

  const clearCompleted = useTauriMutation<number, { deleteFiles: boolean }>(
    "download_clear_completed",
    {
      invalidateKeys: INVALIDATE_KEYS,
      onSuccess: (count) => {
        toast.success(t("downloads.toast.clearedCompleted", { count }));
      },
      onError: (err) => {
        toast.error(t("downloads.toast.clearError", { error: err.message }));
      },
    },
  );

  const clearFailed = useTauriMutation<number, { deleteFiles: boolean }>("download_clear_failed", {
    invalidateKeys: INVALIDATE_KEYS,
    onSuccess: (count) => {
      toast.success(t("downloads.toast.clearedFailed", { count }));
    },
    onError: (err) => {
      toast.error(t("downloads.toast.clearError", { error: err.message }));
    },
  });

  const cancellingRef = useRef(false);
  const handleCancelSelected = async () => {
    if (cancellingRef.current) return;
    cancellingRef.current = true;
    const snapshot = [...selectedDownloadIds];
    try {
      const results = await Promise.allSettled(
        snapshot.map((id) => cancelDownload.mutateAsync({ id: Number(id) })),
      );
      const failedIds = snapshot.filter((_, i) => results[i].status === "rejected");
      const currentIds = useUiStore.getState().selectedDownloadIds;
      const unchanged =
        currentIds.length === snapshot.length && currentIds.every((id, i) => id === snapshot[i]);
      if (unchanged) {
        if (failedIds.length === 0) clearSelection();
        else setSelectedDownloadIds(failedIds);
      }
    } finally {
      cancellingRef.current = false;
    }
  };

  const hasSelection = selectedDownloadIds.length > 0;

  // Subscribes to the shared cache entry so the button enabled/disabled state
  // reactively tracks state transitions. Mirrors the staleTime used by the
  // primary consumer in DownloadsView so the two reads share a single request.
  const { data: counts } = useTauriQuery<Record<string, number>>(
    "download_count_by_state",
    undefined,
    { queryKey: downloadQueries.countByState(), staleTime: 2000 },
  );
  const completedCount = counts?.Completed ?? 0;
  const errorCount = counts?.Error ?? 0;

  const [dialogTarget, setDialogTarget] = useState<ClearDownloadsTarget | null>(null);
  const dialogOpen = dialogTarget !== null;
  const dialogCount = dialogTarget === "completed" ? completedCount : errorCount;

  const [moveDialogOpen, setMoveDialogOpen] = useState(false);
  // Surface the first selected download's destination so the dialog can show
  // a "current location" hint and seed the OS folder picker near the file.
  // The query is cheap (cached, gated on a non-empty id) and avoids piping
  // destinationPath through the lightweight DownloadView read model.
  const firstSelectedId = selectedDownloadIds[0] ?? "";
  const { data: firstSelectedDetail } = useDownloadDetail(firstSelectedId);
  const moveDialogCurrentPath = firstSelectedDetail?.destinationPath;

  const handleMoveConfirm = async (destination: string) => {
    await moveDownloads.mutateAsync({
      ids: selectedDownloadIds.map((id) => Number(id)),
      newDestinationDir: destination,
    });
  };

  const handleDialogConfirm = async (deleteFiles: boolean) => {
    if (dialogTarget === "completed") {
      await clearCompleted.mutateAsync({ deleteFiles });
    } else if (dialogTarget === "error") {
      await clearFailed.mutateAsync({ deleteFiles });
    }
  };

  return (
    <div
      className={`flex items-center gap-2 min-h-[36px] ${hasSelection ? "rounded-md bg-muted/50 px-3 py-1" : ""}`}
    >
      {hasSelection ? (
        <>
          <span className="text-sm text-muted-foreground">
            {t("downloads.selectedCount", { count: selectedDownloadIds.length })}
          </span>
          <Button variant="ghost" size="sm" onClick={handleCancelSelected}>
            <X className="mr-1 h-4 w-4" />
            {t("downloads.actions.cancelSelected")}
          </Button>
          <Button variant="ghost" size="sm" onClick={() => setMoveDialogOpen(true)}>
            <FolderInput className="mr-1 h-4 w-4" />
            {t("downloads.actions.moveSelected")}
          </Button>
          <Button variant="ghost" size="sm" onClick={clearSelection}>
            {t("common.clear")}
          </Button>
        </>
      ) : (
        <>
          <Button variant="ghost" size="sm" onClick={() => pauseAll.mutate()}>
            <Pause className="mr-1 h-4 w-4" />
            {t("downloads.actions.pauseAll")}
          </Button>
          <Button variant="ghost" size="sm" onClick={() => resumeAll.mutate()}>
            <Play className="mr-1 h-4 w-4" />
            {t("downloads.actions.resumeAll")}
          </Button>

          <Separator orientation="vertical" className="mx-1 h-4" />

          <Button
            variant="ghost"
            size="sm"
            disabled={completedCount === 0}
            onClick={() => setDialogTarget("completed")}
          >
            <CheckCheck className="mr-1 h-4 w-4" />
            {t("downloads.actions.clearCompleted")}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            disabled={errorCount === 0}
            onClick={() => setDialogTarget("error")}
          >
            <XCircle className="mr-1 h-4 w-4" />
            {t("downloads.actions.clearFailed")}
          </Button>
        </>
      )}

      {dialogTarget !== null && (
        <ClearDownloadsDialog
          open={dialogOpen}
          onOpenChange={(o) => !o && setDialogTarget(null)}
          targetState={dialogTarget}
          count={dialogCount}
          onConfirm={handleDialogConfirm}
        />
      )}

      <MoveDialog
        open={moveDialogOpen}
        onOpenChange={setMoveDialogOpen}
        count={selectedDownloadIds.length}
        currentPath={moveDialogCurrentPath}
        onConfirm={handleMoveConfirm}
      />
    </div>
  );
}
