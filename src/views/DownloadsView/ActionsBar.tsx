import { useRef, useState } from 'react';
import { CheckCheck, Pause, Play, X, XCircle } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useQueryClient } from '@tanstack/react-query';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { useTauriMutation } from '@/api/hooks';
import { downloadQueries } from '@/api/queries';
import { useUiStore } from '@/stores/uiStore';
import { toast } from '@/lib/toast';
import {
  ClearDownloadsDialog,
  type ClearDownloadsTarget,
} from './ClearDownloadsDialog';

const INVALIDATE_KEYS = [
  downloadQueries.lists(),
  downloadQueries.countByState(),
] as const;

export function ActionsBar() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const selectedDownloadIds = useUiStore((s) => s.selectedDownloadIds);
  const setSelectedDownloadIds = useUiStore((s) => s.setSelectedDownloadIds);
  const clearSelection = useUiStore((s) => s.clearSelection);

  const pauseAll = useTauriMutation<void, void>('download_pause_all', {
    invalidateKeys: INVALIDATE_KEYS,
  });

  const resumeAll = useTauriMutation<void, void>('download_resume_all', {
    invalidateKeys: INVALIDATE_KEYS,
  });

  const cancelDownload = useTauriMutation<void, { id: number }>('download_cancel', {
    invalidateKeys: INVALIDATE_KEYS,
  });

  const clearCompleted = useTauriMutation<number, { deleteFiles: boolean }>(
    'download_clear_completed',
    {
      invalidateKeys: INVALIDATE_KEYS,
      onSuccess: (count) => {
        toast.success(t('downloads.toast.clearedCompleted', { count }));
      },
      onError: (err) => {
        toast.error(t('downloads.toast.clearError', { error: err.message }));
      },
    },
  );

  const clearFailed = useTauriMutation<number, { deleteFiles: boolean }>(
    'download_clear_failed',
    {
      invalidateKeys: INVALIDATE_KEYS,
      onSuccess: (count) => {
        toast.success(t('downloads.toast.clearedFailed', { count }));
      },
      onError: (err) => {
        toast.error(t('downloads.toast.clearError', { error: err.message }));
      },
    },
  );

  const cancellingRef = useRef(false);
  const handleCancelSelected = async () => {
    if (cancellingRef.current) return;
    cancellingRef.current = true;
    const snapshot = [...selectedDownloadIds];
    try {
      const results = await Promise.allSettled(
        snapshot.map((id) => cancelDownload.mutateAsync({ id: Number(id) })),
      );
      const failedIds = snapshot.filter((_, i) => results[i].status === 'rejected');
      const currentIds = useUiStore.getState().selectedDownloadIds;
      const unchanged =
        currentIds.length === snapshot.length
        && currentIds.every((id, i) => id === snapshot[i]);
      if (unchanged) {
        if (failedIds.length === 0) clearSelection();
        else setSelectedDownloadIds(failedIds);
      }
    } finally {
      cancellingRef.current = false;
    }
  };

  const hasSelection = selectedDownloadIds.length > 0;

  const counts =
    queryClient.getQueryData<Record<string, number>>(
      downloadQueries.countByState(),
    ) ?? {};
  const completedCount = counts.Completed ?? 0;
  const errorCount = counts.Error ?? 0;

  const [dialogTarget, setDialogTarget] =
    useState<ClearDownloadsTarget | null>(null);
  const dialogOpen = dialogTarget !== null;
  const dialogCount = dialogTarget === 'completed' ? completedCount : errorCount;

  const handleDialogConfirm = async (deleteFiles: boolean) => {
    if (dialogTarget === 'completed') {
      await clearCompleted.mutateAsync({ deleteFiles });
    } else if (dialogTarget === 'error') {
      await clearFailed.mutateAsync({ deleteFiles });
    }
  };

  return (
    <div
      className={`flex items-center gap-2 min-h-[36px] ${hasSelection ? 'rounded-md bg-muted/50 px-3 py-1' : ''}`}
    >
      {hasSelection ? (
        <>
          <span className="text-sm text-muted-foreground">
            {t('downloads.selectedCount', { count: selectedDownloadIds.length })}
          </span>
          <Button variant="ghost" size="sm" onClick={handleCancelSelected}>
            <X className="mr-1 h-4 w-4" />
            {t('downloads.actions.cancelSelected')}
          </Button>
          <Button variant="ghost" size="sm" onClick={clearSelection}>
            {t('common.clear')}
          </Button>
        </>
      ) : (
        <>
          <Button variant="ghost" size="sm" onClick={() => pauseAll.mutate()}>
            <Pause className="mr-1 h-4 w-4" />
            {t('downloads.actions.pauseAll')}
          </Button>
          <Button variant="ghost" size="sm" onClick={() => resumeAll.mutate()}>
            <Play className="mr-1 h-4 w-4" />
            {t('downloads.actions.resumeAll')}
          </Button>

          <Separator orientation="vertical" className="mx-1 h-4" />

          <Button
            variant="ghost"
            size="sm"
            disabled={completedCount === 0}
            onClick={() => setDialogTarget('completed')}
          >
            <CheckCheck className="mr-1 h-4 w-4" />
            {t('downloads.actions.clearCompleted')}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            disabled={errorCount === 0}
            onClick={() => setDialogTarget('error')}
          >
            <XCircle className="mr-1 h-4 w-4" />
            {t('downloads.actions.clearFailed')}
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
    </div>
  );
}
