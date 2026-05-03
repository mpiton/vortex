import { RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";

interface PluginsHeaderProps {
  enabledCount: number;
  onRefresh: () => void;
  isRefreshing: boolean;
}

export function PluginsHeader({ enabledCount, onRefresh, isRefreshing }: PluginsHeaderProps) {
  const { t } = useTranslation();

  return (
    <div className="h-14 px-6 flex items-center justify-between bg-surface border-b border-border shrink-0">
      <div className="flex items-center gap-5">
        <span className="text-sm font-semibold text-foreground">{t("plugins.title")}</span>
        <div className="flex items-center gap-3 text-[11px] text-text-dim">
          <span className="flex items-center gap-1.5">
            <span
              data-testid="plugins-enabled-count"
              className="font-semibold text-success tabular-nums"
            >
              {enabledCount}
            </span>
            {t("plugins.stats.enabled", { count: enabledCount })}
          </span>
        </div>
      </div>

      <div className="flex items-center gap-2">
        <Button
          variant="outline"
          size="sm"
          onClick={onRefresh}
          disabled={isRefreshing}
          className="h-7 text-[11px] gap-1.5"
        >
          <RefreshCw className={`h-3 w-3 ${isRefreshing ? "animate-spin" : ""}`} />
          {t("plugins.action.refresh")}
        </Button>
      </div>
    </div>
  );
}
