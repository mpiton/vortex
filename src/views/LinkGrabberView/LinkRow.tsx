import { Loader2, Check, X, AlertCircle, HelpCircle, Lock, RotateCcw, Copy } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Checkbox } from "@/components/ui/checkbox";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { formatBytes } from "@/lib/format";
import { useLinkGrabberStore } from "@/stores/linkGrabberStore";
import type { DuplicateSource, LinkStatus, ResolvedLink } from "./types";

interface LinkRowProps {
  link: ResolvedLink;
  selected: boolean;
  onSelect: () => void;
  onMediaClick?: (link: ResolvedLink) => void;
  /**
   * Re-issue `link_check_online` for a single URL. Surfaced as a
   * retry button when the row's terminal status is `unknown` so
   * the user can recover from a transient timeout / network blip
   * without re-pasting the whole batch.
   */
  onRetry?: (url: string) => void;
}

const statusIconMap: Record<LinkStatus, React.ReactElement> = {
  checking: <Loader2 className="h-4 w-4 animate-spin text-blue-500" aria-label="checking" />,
  online: <Check className="h-4 w-4 text-green-600" aria-label="online" />,
  offline: <X className="h-4 w-4 text-red-500" aria-label="offline" />,
  premiumOnly: <Lock className="h-4 w-4 text-orange-500" aria-label="premium-only" />,
  unknown: <HelpCircle className="h-4 w-4 text-muted-foreground" aria-label="unknown" />,
  error: <AlertCircle className="h-4 w-4 text-yellow-500" aria-label="error" />,
};

const statusBadgeColor: Record<LinkStatus, string> = {
  checking: "bg-blue-500/15 text-blue-700 dark:text-blue-300",
  online: "bg-green-500/15 text-green-700 dark:text-green-300",
  offline: "bg-red-500/15 text-red-700 dark:text-red-300",
  premiumOnly: "bg-orange-500/15 text-orange-700 dark:text-orange-300",
  unknown: "bg-muted text-muted-foreground",
  error: "bg-yellow-500/15 text-yellow-700 dark:text-yellow-300",
};

const duplicateLabelKeyMap: Record<DuplicateSource, string> = {
  active: "linkGrabber.duplicate.active",
  history: "linkGrabber.duplicate.history",
};

export function LinkRow({ link, selected, onSelect, onMediaClick, onRetry }: LinkRowProps) {
  const { t } = useTranslation();
  const liveStatus = useLinkGrabberStore((s) => s.statuses[link.originalUrl]);
  const effectiveStatus: LinkStatus = liveStatus?.kind ?? link.status;
  const showRetry = effectiveStatus === "unknown" && onRetry !== undefined;

  const duplicate = link.duplicate?.isDuplicate ? link.duplicate : null;
  const duplicateLabel = duplicate?.source ? t(duplicateLabelKeyMap[duplicate.source]) : null;
  const duplicateTooltip =
    duplicate?.existingFilename && duplicateLabel
      ? t("linkGrabber.duplicate.tooltipWithFilename", {
          label: duplicateLabel,
          filename: duplicate.existingFilename,
        })
      : (duplicateLabel ?? "");

  return (
    <div
      className={`flex items-center gap-3 rounded p-2 transition-colors hover:bg-muted ${
        selected ? "bg-accent/20" : ""
      }`}
      data-testid={`link-row-${link.originalUrl}`}
      data-status={effectiveStatus}
      data-duplicate={duplicate?.source ?? "none"}
    >
      <Checkbox checked={selected} onCheckedChange={onSelect} aria-label="Select link" />
      <span
        className={`flex h-5 items-center gap-1 rounded px-1.5 text-xs font-medium ${statusBadgeColor[effectiveStatus]}`}
        title={effectiveStatus}
      >
        {statusIconMap[effectiveStatus]}
      </span>
      {duplicate && (
        <Tooltip>
          <TooltipTrigger asChild>
            <span
              tabIndex={0}
              role="status"
              aria-label={duplicateTooltip}
              data-testid={`link-row-duplicate-${link.originalUrl}`}
              className="flex h-5 shrink-0 items-center gap-1 rounded bg-orange-500/15 px-1.5 text-xs font-medium text-orange-700 dark:text-orange-300"
            >
              <Copy className="h-3 w-3" aria-hidden="true" />
              {duplicateLabel}
            </span>
          </TooltipTrigger>
          <TooltipContent>{duplicateTooltip}</TooltipContent>
        </Tooltip>
      )}
      <Tooltip>
        <TooltipTrigger asChild>
          <span tabIndex={0} className="min-w-0 flex-1 truncate text-sm font-semibold">
            {link.filename || link.originalUrl}
          </span>
        </TooltipTrigger>
        <TooltipContent>{link.originalUrl}</TooltipContent>
      </Tooltip>
      <span className="shrink-0 text-xs text-muted-foreground">{link.moduleName}</span>
      {link.sizeBytes !== null && (
        <span className="shrink-0 text-xs text-muted-foreground">
          {formatBytes(link.sizeBytes)}
        </span>
      )}
      {link.isMedia && (
        <Button
          variant="outline"
          size="sm"
          className="h-6 px-2 text-xs"
          onClick={(e) => {
            e.stopPropagation();
            onMediaClick?.(link);
          }}
        >
          {link.mediaType ?? "Media"}
        </Button>
      )}
      {showRetry && (
        <Button
          variant="ghost"
          size="sm"
          className="h-6 px-2 text-xs"
          aria-label="retry-link-check"
          onClick={(e) => {
            e.stopPropagation();
            onRetry?.(link.originalUrl);
          }}
        >
          <RotateCcw className="h-3.5 w-3.5" />
        </Button>
      )}
    </div>
  );
}
