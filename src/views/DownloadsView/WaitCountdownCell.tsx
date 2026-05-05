import { SkipForward } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useTauriMutation } from "@/api/hooks";
import { downloadQueries } from "@/api/queries";
import { useCountdown } from "@/hooks/useCountdown";
import { useDownloadStore } from "@/stores/downloadStore";

interface WaitCountdownCellProps {
  downloadId: string;
}

/**
 * Inline countdown rendered in the ETA column whenever a row is parked
 * in the `Waiting` state. Falls through to a static "Waiting…" hint
 * when the wait ticket hasn't been received yet (e.g. the row was
 * already in `Waiting` before the page mounted, before any
 * `download-waiting-started` event arrived). The skip button calls
 * `download_skip_wait` to short-circuit the cooldown.
 */
export function WaitCountdownCell({ downloadId }: WaitCountdownCellProps) {
  const { t } = useTranslation();
  const ticket = useDownloadStore((s) => s.waitMap[downloadId]);
  const { label } = useCountdown(ticket?.untilUnixMs ?? null);
  const skipMut = useTauriMutation<unknown, { id: number }>("download_skip_wait", {
    invalidateKeys: [downloadQueries.all()],
  });

  if (!ticket) {
    return (
      <span className="text-xs text-muted-foreground italic">
        {t("downloads.wait.unknown", { defaultValue: "Waiting…" })}
      </span>
    );
  }

  return (
    <div className="flex items-center gap-2">
      <span className="font-mono text-xs text-orange-600">
        {t("downloads.wait.remaining", {
          defaultValue: "Wait {{label}}",
          label,
        })}
      </span>
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            type="button"
            variant="ghost"
            size="icon"
            className="size-6"
            onClick={(event) => {
              event.stopPropagation();
              skipMut.mutate({ id: Number(downloadId) });
            }}
            aria-label={t("downloads.wait.skip", { defaultValue: "Skip wait" })}
          >
            <SkipForward className="size-3.5" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          <p className="max-w-[240px] text-xs">
            {ticket.reason || t("downloads.wait.reason", { defaultValue: "Hoster cooldown" })}
          </p>
        </TooltipContent>
      </Tooltip>
    </div>
  );
}
