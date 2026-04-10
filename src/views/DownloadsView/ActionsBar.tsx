import { useRef } from 'react';
import { Pause, Play, X } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { useTauriMutation } from '@/api/hooks';
import { downloadQueries } from '@/api/queries';
import { useUiStore } from '@/stores/uiStore';

const INVALIDATE_KEYS = [
  downloadQueries.lists(),
  downloadQueries.countByState(),
] as const;

export function ActionsBar() {
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
            {selectedDownloadIds.length} selected
          </span>
          <Button variant="ghost" size="sm" onClick={handleCancelSelected}>
            <X className="mr-1 h-4 w-4" />
            Cancel Selected
          </Button>
          <Button variant="ghost" size="sm" onClick={clearSelection}>
            Clear
          </Button>
        </>
      ) : (
        <>
          <Button variant="ghost" size="sm" onClick={() => pauseAll.mutate()}>
            <Pause className="mr-1 h-4 w-4" />
            Pause All
          </Button>
          <Button variant="ghost" size="sm" onClick={() => resumeAll.mutate()}>
            <Play className="mr-1 h-4 w-4" />
            Resume All
          </Button>
        </>
      )}
    </div>
  );
}
