import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import {
  ArrowRightLeft,
  ChevronDown,
  ChevronRight,
  Folder,
  Key,
  Pencil,
  Play,
  Pause,
  Trash2,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { Switch } from "@/components/ui/switch";
import { formatBytes } from "@/lib/format";
import type { DownloadView } from "@/types/download";
import type { PackageView } from "@/types/package";
import { PackageDownloadRow } from "./PackageDownloadRow";

export interface PendingMove {
  downloadId: number;
  fromPackageId: string;
  fileName: string;
}

export interface PackageRowActions {
  toggleExpand: (id: string) => void;
  rename: (pkg: PackageView) => void;
  setPassword: (pkg: PackageView) => void;
  changeFolder: (pkg: PackageView) => void;
  deletePackage: (pkg: PackageView) => void;
  toggleAutoExtract: (pkg: PackageView) => void;
  setPriority: (pkg: PackageView, priority: number) => void;
  pauseAll: (pkg: PackageView, downloads: DownloadView[]) => void;
  startAll: (pkg: PackageView, downloads: DownloadView[]) => void;
  beginDragDownload: (download: DownloadView, fromPackageId: string) => void;
  endDragDownload: () => void;
  dropDownload: (toPackageId: string, e: React.DragEvent) => void;
  selectForMove: (download: DownloadView, fromPackageId: string) => void;
  cancelMove: () => void;
  executeMove: (toPackage: PackageView) => void;
}

interface PackageRowProps {
  pkg: PackageView;
  expanded: boolean;
  childrenLoading: boolean;
  childrenError: Error | null;
  childDownloads: DownloadView[] | null;
  pendingMove: PendingMove | null;
  actions: PackageRowActions;
}

const PRIORITIES = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] as const;

export function PackageRow({
  pkg,
  expanded,
  childrenLoading,
  childrenError,
  childDownloads,
  pendingMove,
  actions,
}: PackageRowProps) {
  const { t } = useTranslation();
  const sourceLabelKey = useMemo(
    () => `packages.filter.${pkg.sourceType}`,
    [pkg.sourceType],
  );

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    actions.dropDownload(pkg.id, e);
  };

  const isMoveTarget =
    pendingMove !== null && pendingMove.fromPackageId !== pkg.id;

  return (
    <div
      data-testid={`package-row-${pkg.id}-dropzone`}
      onDragOver={(e) => {
        e.preventDefault();
        e.dataTransfer.dropEffect = "move";
      }}
      onDrop={handleDrop}
      aria-label={t("packages.drag.dropZoneAriaLabel")}
    >
      <div
        data-testid={`package-row-${pkg.id}`}
        className="flex flex-wrap items-center gap-3 border-b px-4 py-3 hover:bg-muted/40"
      >
        <Button
          variant="ghost"
          size="sm"
          data-testid={`package-row-${pkg.id}-toggle`}
          aria-label={expanded ? t("packages.row.collapse") : t("packages.row.expand")}
          onClick={() => actions.toggleExpand(pkg.id)}
        >
          {expanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
        </Button>

        <button
          type="button"
          data-testid={`package-row-${pkg.id}-rename`}
          onClick={() => actions.rename(pkg)}
          className="flex min-w-0 items-center gap-2 truncate text-left text-sm font-medium"
          aria-label={t("packages.row.renameAriaLabel")}
        >
          <span className="truncate">{pkg.name}</span>
          <Pencil className="h-3 w-3 text-muted-foreground" aria-hidden />
        </button>

        <Badge variant="outline">{t(sourceLabelKey)}</Badge>

        <span className="text-xs text-muted-foreground">
          {t("packages.row.files", { count: Number(pkg.downloadsCount) })}
        </span>
        <span className="text-xs text-muted-foreground">{formatBytes(pkg.totalBytes)}</span>

        <div className="flex min-w-[120px] flex-col gap-1">
          <Progress value={pkg.progressPercent} className="h-1.5 w-32" />
          <span className="text-[10px] text-muted-foreground">
            {pkg.progressPercent.toFixed(0)}%
          </span>
        </div>

        <Button
          size="sm"
          variant="outline"
          data-testid={`package-row-${pkg.id}-folder`}
          onClick={() => actions.changeFolder(pkg)}
          aria-label={t("packages.actions.changeFolder")}
        >
          <Folder className="mr-1 h-3 w-3" />
          <span className="max-w-[120px] truncate">
            {pkg.folderPath ?? t("packages.row.noFolder")}
          </span>
        </Button>

        <Button
          size="sm"
          variant="outline"
          data-testid={`package-row-${pkg.id}-password`}
          onClick={() => actions.setPassword(pkg)}
          aria-label={t("packages.actions.setPassword")}
        >
          <Key className="h-3 w-3" />
        </Button>

        <label className="flex items-center gap-1 text-xs">
          <span className="text-muted-foreground">{t("packages.row.autoExtract")}</span>
          <Switch
            data-testid={`package-row-${pkg.id}-auto-extract`}
            checked={pkg.autoExtract}
            onCheckedChange={() => actions.toggleAutoExtract(pkg)}
          />
        </label>

        <label className="flex items-center gap-1 text-xs">
          <span className="text-muted-foreground">{t("packages.row.priority")}</span>
          <select
            data-testid={`package-row-${pkg.id}-priority`}
            value={pkg.priority}
            onChange={(e) => actions.setPriority(pkg, Number(e.target.value))}
            className="rounded border bg-background px-1 py-0.5 text-xs"
          >
            {PRIORITIES.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </select>
        </label>

        <Button
          size="sm"
          variant="outline"
          data-testid={`package-row-${pkg.id}-pause-all`}
          onClick={() => actions.pauseAll(pkg, childDownloads ?? [])}
          disabled={!expanded || !childDownloads || childDownloads.length === 0}
        >
          <Pause className="h-3 w-3" />
          <span className="ml-1 hidden sm:inline">{t("packages.actions.pauseAll")}</span>
        </Button>

        <Button
          size="sm"
          variant="outline"
          data-testid={`package-row-${pkg.id}-start-all`}
          onClick={() => actions.startAll(pkg, childDownloads ?? [])}
          disabled={!expanded || !childDownloads || childDownloads.length === 0}
        >
          <Play className="h-3 w-3" />
          <span className="ml-1 hidden sm:inline">{t("packages.actions.startAll")}</span>
        </Button>

        <Button
          size="sm"
          variant="ghost"
          data-testid={`package-row-${pkg.id}-delete`}
          onClick={() => actions.deletePackage(pkg)}
          aria-label={t("packages.actions.delete")}
          className="text-destructive hover:text-destructive"
        >
          <Trash2 className="h-3 w-3" />
        </Button>

        {isMoveTarget && (
          <Button
            size="sm"
            variant="default"
            data-testid={`package-row-${pkg.id}-move-target`}
            onClick={() => actions.executeMove(pkg)}
            aria-label={t("packages.move.targetAriaLabel", { name: pkg.name })}
          >
            <ArrowRightLeft className="mr-1 h-3 w-3" />
            <span className="hidden sm:inline">{t("packages.move.target")}</span>
          </Button>
        )}
      </div>

      {expanded && (
        <div data-testid={`package-row-${pkg.id}-children`}>
          {childrenLoading && (
            <div className="px-6 py-3 text-xs text-muted-foreground">
              {t("packages.row.loadingChildren")}
            </div>
          )}
          {childrenError && (
            <div className="px-6 py-3 text-xs text-destructive">{childrenError.message}</div>
          )}
          {!childrenLoading && childDownloads !== null && childDownloads.length === 0 && (
            <div className="px-6 py-3 text-xs text-muted-foreground">
              {t("packages.row.noChildren")}
            </div>
          )}
          {childDownloads?.map((d) => (
            <PackageDownloadRow
              key={d.id}
              download={d}
              packageId={pkg.id}
              isPendingMove={
                pendingMove !== null && pendingMove.downloadId === Number(d.id)
              }
              onDragStart={actions.beginDragDownload}
              onDragEnd={actions.endDragDownload}
              onSelectForMove={actions.selectForMove}
              onCancelMove={actions.cancelMove}
            />
          ))}
        </div>
      )}
    </div>
  );
}
