import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { useTranslation } from "react-i18next";
import type { FilterType, FilterConfig } from "./types";

const FILTERS: FilterConfig[] = [
  { type: "all", labelKey: "downloads.filters.all" },
  { type: "active", labelKey: "downloads.filters.active", states: ["Downloading", "Queued"] },
  { type: "queued", labelKey: "downloads.filters.queued", states: ["Queued"] },
  { type: "done", labelKey: "downloads.filters.done", states: ["Completed"] },
  { type: "failed", labelKey: "downloads.filters.failed", states: ["Error", "Retry"] },
];

interface FilterBarProps {
  activeFilter: FilterType;
  onFilterChange: (f: FilterType) => void;
  counts: Record<string, number> | undefined;
}

function getFilterCount(filter: FilterConfig, counts: Record<string, number> | undefined): number {
  if (!counts) return 0;
  if (filter.type === "all") {
    return counts.total ?? Object.values(counts).reduce((sum, v) => sum + v, 0);
  }
  if (!filter.states) return 0;
  return filter.states.reduce((sum, state) => sum + (counts[state] ?? 0), 0);
}

export function FilterBar({ activeFilter, onFilterChange, counts }: FilterBarProps) {
  const { t } = useTranslation();

  return (
    <div className="flex gap-1.5 border-b pb-2">
      {FILTERS.map((filter) => (
        <Button
          key={filter.type}
          variant={activeFilter === filter.type ? "default" : "ghost"}
          size="sm"
          onClick={() => onFilterChange(filter.type)}
        >
          {t(filter.labelKey)}
          <Badge variant="secondary" className="ml-1.5">
            {getFilterCount(filter, counts)}
          </Badge>
        </Button>
      ))}
    </div>
  );
}
