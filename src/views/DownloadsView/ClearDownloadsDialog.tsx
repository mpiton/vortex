import { useEffect, useState } from 'react';
import { AlertTriangle } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogDescription,
} from '@/components/ui/dialog';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';

export type ClearDownloadsTarget = 'completed' | 'error';

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  targetState: ClearDownloadsTarget;
  count: number;
  onConfirm: (deleteFiles: boolean) => Promise<void> | void;
}

export function ClearDownloadsDialog({
  open,
  onOpenChange,
  targetState,
  count,
  onConfirm,
}: Props) {
  const { t } = useTranslation();
  const [deleteFiles, setDeleteFiles] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (open) setDeleteFiles(false);
  }, [open]);

  const titleKey =
    targetState === 'completed'
      ? 'downloads.clearDialog.titleCompleted'
      : 'downloads.clearDialog.titleFailed';

  const confirmLabel = deleteFiles
    ? t('downloads.clearDialog.confirmWithFiles')
    : t('downloads.clearDialog.confirm');

  const handleConfirm = async () => {
    if (submitting) return;
    setSubmitting(true);
    try {
      await onConfirm(deleteFiles);
      onOpenChange(false);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t(titleKey, { count })}</DialogTitle>
          <DialogDescription>
            {t('downloads.clearDialog.description')}
          </DialogDescription>
        </DialogHeader>

        <label className="flex items-center gap-2 text-sm">
          <Checkbox
            checked={deleteFiles}
            onCheckedChange={(v) => setDeleteFiles(Boolean(v))}
            aria-label={t('downloads.clearDialog.deleteFilesLabel')}
          />
          <span>{t('downloads.clearDialog.deleteFilesLabel')}</span>
        </label>

        {deleteFiles && (
          <div
            role="alert"
            className="rounded-md border border-destructive/40 bg-destructive/10 p-3 flex gap-2 items-start"
          >
            <AlertTriangle
              className="h-5 w-5 shrink-0 text-destructive"
              aria-hidden="true"
            />
            <div>
              <p className="font-semibold text-destructive">
                {t('downloads.clearDialog.warningTitle')}
              </p>
              <p className="text-sm text-destructive/90">
                {t('downloads.clearDialog.warningBody')}
              </p>
            </div>
          </div>
        )}

        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={submitting}
          >
            {t('downloads.clearDialog.cancel')}
          </Button>
          <Button
            variant={deleteFiles ? 'destructive' : 'default'}
            onClick={handleConfirm}
            disabled={submitting}
          >
            {confirmLabel}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
