import { useCallback } from "react";
import { Download, FileDown, FileJson, Search } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { HistoryFilterType } from "./filterEntries";

export interface HistoryHeaderProps {
  search: string;
  onSearchChange: (next: string) => void;
  filter: HistoryFilterType;
  onFilterChange: (next: HistoryFilterType) => void;
  counts: Record<HistoryFilterType, number>;
  onExport: (format: "csv" | "json") => void | Promise<void>;
  exportDisabled?: boolean;
}

const FILTERS: HistoryFilterType[] = ["all", "completed", "failed", "cancelled"];

export function HistoryHeader({
  search,
  onSearchChange,
  filter,
  onFilterChange,
  counts,
  onExport,
  exportDisabled = false,
}: HistoryHeaderProps) {
  const { t } = useTranslation();

  const handleExportCsv = useCallback(() => {
    void onExport("csv");
  }, [onExport]);
  const handleExportJson = useCallback(() => {
    void onExport("json");
  }, [onExport]);

  return (
    <div className="flex flex-col gap-3 border-b pb-3">
      <div className="flex items-center gap-2">
        <div className="relative flex-1">
          <Search className="absolute left-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            data-shortcut-target="history-search"
            value={search}
            onChange={(e) => onSearchChange(e.target.value)}
            placeholder={t("history.searchPlaceholder")}
            aria-label={t("history.searchAriaLabel")}
            className="pl-9"
          />
        </div>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="outline"
              size="sm"
              disabled={exportDisabled}
              aria-label={t("history.export.trigger")}
            >
              <Download className="mr-2 size-3.5" />
              {t("history.export.trigger")}
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onClick={handleExportCsv}>
              <FileDown className="size-3.5" />
              {t("history.export.csv")}
            </DropdownMenuItem>
            <DropdownMenuItem onClick={handleExportJson}>
              <FileJson className="size-3.5" />
              {t("history.export.json")}
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <div className="flex gap-1.5" role="tablist" aria-label={t("history.filters.ariaLabel")}>
        {FILTERS.map((type) => (
          <Button
            key={type}
            role="tab"
            aria-selected={filter === type}
            variant={filter === type ? "default" : "ghost"}
            size="sm"
            onClick={() => onFilterChange(type)}
          >
            {t(`history.filters.${type}`)}
            <Badge variant="secondary" className="ml-1.5">
              {counts[type]}
            </Badge>
          </Button>
        ))}
      </div>
    </div>
  );
}
