import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useTauriMutation } from "@/api/hooks";
import { downloadQueries, historyQueries } from "@/api/queries";
import { OverwriteDialog, type OverwriteDecision } from "@/components/ui/OverwriteDialog";
import { toast } from "@/lib/toast";

export type RedownloadSourceKind = "download" | "history";
export type RedownloadOverwriteMode = "overwrite" | "rename";

export interface RedownloadIpcArgs extends Record<string, unknown> {
  sourceKind: RedownloadSourceKind;
  sourceId: number;
  overwriteMode: RedownloadOverwriteMode | null;
}

export type RedownloadOutcome =
  | { kind: "created"; id: number }
  | { kind: "fileExists"; originalPath: string; suggestedPath: string };

interface PendingPrompt {
  sourceKind: RedownloadSourceKind;
  sourceId: number;
  originalPath: string;
  suggestedPath: string;
}

interface UseRedownloadOptions {
  /** i18n key used for the success toast (defaults to downloads namespace). */
  successToastKey?: string;
  /** i18n key used for the error toast (defaults to downloads namespace). */
  errorToastKey?: string;
}

/**
 * Redownload trigger paired with the overwrite dialog.
 *
 * Usage:
 *   const { trigger, dialog } = useRedownload();
 *   // ... somewhere in JSX:
 *   <>{dialog}</>
 *   // ... somewhere in a handler:
 *   trigger('download', Number(id));
 */
export function useRedownload(options: UseRedownloadOptions = {}) {
  const { t } = useTranslation();
  const [pending, setPending] = useState<PendingPrompt | null>(null);

  const invalidateKeys = useMemo(
    () => [downloadQueries.lists(), downloadQueries.countByState(), historyQueries.lists()],
    [],
  );

  const successKey = options.successToastKey ?? "downloads.table.toast.redownloadSuccess";
  const errorKey = options.errorToastKey ?? "downloads.table.toast.redownloadError";

  const mutation = useTauriMutation<RedownloadOutcome, RedownloadIpcArgs>("download_redownload", {
    invalidateKeys,
    errorMessage: () => t(errorKey),
  });

  const handleOutcome = useCallback(
    (
      outcome: RedownloadOutcome | null | undefined,
      context: { sourceKind: RedownloadSourceKind; sourceId: number },
    ) => {
      if (!outcome) return;
      if (outcome.kind === "fileExists") {
        setPending({
          sourceKind: context.sourceKind,
          sourceId: context.sourceId,
          originalPath: outcome.originalPath,
          suggestedPath: outcome.suggestedPath,
        });
        return;
      }
      toast.success(t(successKey));
    },
    [successKey, t],
  );

  const trigger = useCallback(
    (sourceKind: RedownloadSourceKind, sourceId: number) => {
      mutation.mutate(
        { sourceKind, sourceId, overwriteMode: null },
        {
          onSuccess: (outcome) => handleOutcome(outcome, { sourceKind, sourceId }),
        },
      );
    },
    [handleOutcome, mutation],
  );

  const handleDecision = useCallback(
    (decision: OverwriteDecision) => {
      if (!pending) return;
      const { sourceKind, sourceId } = pending;
      setPending(null);
      if (decision === "cancel") return;
      mutation.mutate(
        {
          sourceKind,
          sourceId,
          overwriteMode: decision,
        },
        {
          onSuccess: (outcome) => handleOutcome(outcome, { sourceKind, sourceId }),
        },
      );
    },
    [handleOutcome, mutation, pending],
  );

  const dialog = (
    <OverwriteDialog
      open={pending !== null}
      onOpenChange={(open) => {
        if (!open) setPending(null);
      }}
      originalPath={pending?.originalPath ?? ""}
      suggestedPath={pending?.suggestedPath ?? ""}
      onDecision={handleDecision}
    />
  );

  return { trigger, dialog, isPending: mutation.isPending };
}
