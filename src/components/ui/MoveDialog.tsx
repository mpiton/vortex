import { useEffect, useState } from "react";
import { FolderOpen } from "lucide-react";
import { useTranslation } from "react-i18next";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { useBrowseFolder } from "@/hooks/useBrowseFolder";

interface MoveDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** How many downloads will be moved. Drives title pluralisation. */
  count: number;
  /**
   * Path of the first selected download — used as the dialog's "current
   * location" hint and as the default folder for the picker. Optional so the
   * caller can omit it when no representative path exists (e.g. a freshly
   * queued download with no on-disk file yet).
   */
  currentPath?: string;
  /**
   * Resolves with the destination folder. Receives `null` when the dialog
   * is dismissed via cancel/close so the caller can clear pending state.
   */
  onConfirm: (destination: string) => Promise<void> | void;
}

export function MoveDialog({
  open,
  onOpenChange,
  count,
  currentPath,
  onConfirm,
}: MoveDialogProps) {
  const { t } = useTranslation();
  const browseFolder = useBrowseFolder();
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  // Reset selection every time the dialog opens so the user always picks
  // explicitly — never carries stale paths across invocations.
  useEffect(() => {
    if (open) setSelectedPath(null);
  }, [open]);

  const handleBrowse = async () => {
    const defaultDir = deriveDefaultDir(currentPath);
    const picked = await browseFolder(defaultDir);
    if (picked) setSelectedPath(picked);
  };

  const handleConfirm = async () => {
    if (!selectedPath || submitting) return;
    setSubmitting(true);
    try {
      await onConfirm(selectedPath);
      onOpenChange(false);
    } catch {
      // Failure surfaced via the mutation's onError toast — keep the
      // dialog open so the user can retry without re-picking the folder.
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("downloads.moveDialog.title", { count })}</DialogTitle>
          <DialogDescription>
            {t("downloads.moveDialog.description")}
          </DialogDescription>
        </DialogHeader>

        {currentPath && (
          <div className="text-sm">
            <span className="font-medium">
              {t("downloads.moveDialog.currentLabel")}
            </span>
            <p
              className="mt-1 break-all rounded-md bg-muted/50 p-2 font-mono text-xs"
              data-testid="move-current-path"
            >
              {currentPath}
            </p>
          </div>
        )}

        <div className="text-sm">
          <span className="font-medium">
            {t("downloads.moveDialog.destinationLabel")}
          </span>
          <div className="mt-1 flex items-center gap-2">
            <p
              className="flex-1 break-all rounded-md bg-muted/50 p-2 font-mono text-xs"
              data-testid="move-destination-path"
            >
              {selectedPath ?? t("downloads.moveDialog.noFolderSelected")}
            </p>
            <Button
              variant="outline"
              size="sm"
              onClick={handleBrowse}
              disabled={submitting}
            >
              <FolderOpen className="mr-1 h-4 w-4" />
              {t("downloads.moveDialog.browse")}
            </Button>
          </div>
        </div>

        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={submitting}
          >
            {t("downloads.moveDialog.cancel")}
          </Button>
          <Button
            onClick={handleConfirm}
            disabled={!selectedPath || submitting}
          >
            {t("downloads.moveDialog.confirm")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/**
 * Pick a starting directory for the OS folder picker so the user lands close
 * to where the download currently lives instead of the OS default home.
 *
 * Returns the parent of `currentPath` when supplied; the picker treats `null`
 * as "open at last location / OS default" which is the right fallback when
 * the caller has no representative path.
 */
function deriveDefaultDir(currentPath: string | undefined): string | null {
  if (!currentPath) return null;
  // Strip the trailing basename — the picker wants a directory, not a file.
  const lastSep = Math.max(
    currentPath.lastIndexOf("/"),
    currentPath.lastIndexOf("\\"),
  );
  if (lastSep <= 0) return null;
  return currentPath.slice(0, lastSep);
}
