import { useRef } from 'react';
import { Pause, Play, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Button } from '@/components/ui/button';
import { useTauriMutation } from '@/api/hooks';
import { downloadQueries } from '@/api/queries';
import { useUiStore } from '@/stores/uiStore';

const INVALIDATE_KEYS = [
  downloadQueries.lists(),
  downloadQueries.countByState(),
] as const;

export function ActionsBar() {
  const { t } = useTranslation();
  const selectedDownloadIds = useUiStore((s) => s.selectedDownloadIds);
  const setSelectedDownloadIds = useUiStore((s) => s.setSelectedDownloadIds);
  const clearSelection = useUiStore((s) => s.clearSelection);

  const pauseAll = useTauriMutation<void, void>('download_pause_all', {
    invalidateKeys: INVALIDATE_KEYS,
  });

  const resumeAll = useTauriMutation<void, void>('download_resume_all', {
    invalidateKeys: INVALIDATE_KEYS,
  });

  const cancelDownload = useTauriMutation<void, { id: string }>('download_cancel', {
    invalidateKeys: INVALIDATE_KEYS,
  });

  const cancellingRef = useRef(false);

  const handleCancelSelected = async () => {
    if (cancellingRef.current) return;
    cancellingRef.current = true;
    const snapshot = [...selectedDownloadIds];
    try {
      const results = await Promise.allSettled(
        snapshot.map((id) => cancelDownload.mutateAsync({ id })),
      );
      const failedIds = snapshot.filter((_, i) => results[i].status === 'rejected');
      const currentIds = useUiStore.getState().selectedDownloadIds;
      const unchanged = currentIds.length === snapshot.length
        && currentIds.every((id, i) => id === snapshot[i]);
      if (unchanged) {
        if (failedIds.length === 0) {
          clearSelection();
        } else {
          setSelectedDownloadIds(failedIds);
        }
      }
    } finally {
      cancellingRef.current = false;
    }
  };

  const hasSelection = selectedDownloadIds.length > 0;

  return (
    <div className={`flex items-center gap-2 min-h-[36px] ${hasSelection ? 'rounded-md bg-muted/50 px-3 py-1' : ''}`}>
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
        </>
      )}
    </div>
  );
}
