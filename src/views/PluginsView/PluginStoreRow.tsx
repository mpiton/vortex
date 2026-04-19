import { MoreVertical, ArrowUpCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { PluginStoreEntry } from "@/types/plugin-store";

interface PluginStoreRowProps {
  entry: PluginStoreEntry;
  isLocallyDisabled?: boolean;
  onInstall: (name: string) => void;
  onUpdate: (name: string) => void;
  onDisable?: (name: string) => void;
  onEnable?: (name: string) => void;
  onUninstall?: (name: string) => void;
  isInstalling: boolean;
  isUpdating: boolean;
}

const CRAWLER_CATEGORIES = new Set(["crawler", "extractor"]);

function buildMonogram(name: string): string {
  const slug = name.replace(/^vortex-mod-/i, "");
  return slug.slice(0, 2).toUpperCase();
}

function isInstalledLike(status: PluginStoreEntry["status"]): boolean {
  return status === "installed" || status === "update_available" || status === "downgrade";
}

export function PluginStoreRow({
  entry,
  isLocallyDisabled = false,
  onInstall,
  onUpdate,
  onDisable,
  onEnable,
  onUninstall,
  isInstalling,
  isUpdating,
}: PluginStoreRowProps) {
  const { t } = useTranslation();
  const installed = isInstalledLike(entry.status);
  const categoryLabel = t(`plugins.categories.${entry.category}`, {
    defaultValue: entry.category,
  });
  const iconColorClass = CRAWLER_CATEGORIES.has(entry.category)
    ? "bg-accent-light text-accent"
    : "bg-surface-muted text-muted";
  const rowClassName = `flex items-center gap-3 px-4 py-2.5 border-b border-border-soft last:border-0 hover:bg-surface-alt/60 transition-colors${
    isLocallyDisabled ? " opacity-60" : ""
  }`;

  return (
    <div className={rowClassName}>
      <div
        data-testid="plugin-icon"
        className={`w-7 h-7 rounded-md flex items-center justify-center text-[10px] font-semibold shrink-0 ${iconColorClass}`}
      >
        {buildMonogram(entry.name)}
      </div>

      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5 flex-wrap">
          <span className="text-xs font-medium text-foreground truncate">{entry.name}</span>
          {entry.official && (
            <Badge variant="secondary" className="text-[10px] px-1.5 py-0 h-4 shrink-0">
              {t("plugins.badge.official")}
            </Badge>
          )}
          {isLocallyDisabled && (
            <Badge
              variant="outline"
              className="text-[10px] px-1.5 py-0 h-4 shrink-0 border-text-ghost text-text-dim"
            >
              {t("plugins.badge.inactive")}
            </Badge>
          )}
        </div>
        <p className="text-[10px] text-text-dim mt-0.5 truncate">
          {entry.description}
          <span className="mx-1.5">·</span>
          {categoryLabel}
          <span className="mx-1.5">·</span>
          {entry.author}
        </p>
      </div>

      <div className="flex items-center gap-2 shrink-0">
        {entry.status === "update_available" && !isLocallyDisabled && (
          <Button
            size="sm"
            variant="outline"
            onClick={() => onUpdate(entry.name)}
            disabled={isUpdating}
            className="h-7 text-[10px] px-2 gap-1 text-warning border-warning/50 hover:bg-warning/10 hover:text-warning"
          >
            <ArrowUpCircle className="h-3 w-3" />
            v{entry.version}
          </Button>
        )}

        {installed && entry.installedVersion && (
          <span className="text-[10px] text-text-ghost tabular-nums">
            v{entry.installedVersion}
          </span>
        )}

        {entry.status === "not_installed" && (
          <Button
            size="sm"
            variant="outline"
            onClick={() => onInstall(entry.name)}
            disabled={isInstalling}
            className="h-7 text-[10px] px-3"
          >
            {t("plugins.action.install")}
          </Button>
        )}

        {installed && (onDisable || onEnable || onUninstall) && (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                size="icon"
                variant="ghost"
                className="h-7 w-7"
                aria-label={t("plugins.action.more")}
              >
                <MoreVertical className="h-3.5 w-3.5" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {isLocallyDisabled
                ? onEnable && (
                    <DropdownMenuItem onSelect={() => onEnable(entry.name)}>
                      {t("plugins.action.enable")}
                    </DropdownMenuItem>
                  )
                : onDisable && (
                    <DropdownMenuItem onSelect={() => onDisable(entry.name)}>
                      {t("plugins.action.disable")}
                    </DropdownMenuItem>
                  )}
              {((isLocallyDisabled ? onEnable : onDisable) && onUninstall) && (
                <DropdownMenuSeparator />
              )}
              {onUninstall && (
                <DropdownMenuItem
                  variant="destructive"
                  onSelect={() => onUninstall(entry.name)}
                >
                  {t("plugins.action.uninstall")}
                </DropdownMenuItem>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </div>
    </div>
  );
}
