import { useTranslation } from "react-i18next";
import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { PackageSourceType } from "@/types/package";

const FILTER_ORDER: ReadonlyArray<"all" | PackageSourceType> = [
  "all",
  "container",
  "playlist",
  "manual",
  "split_archive",
];

interface PackageToolbarProps {
  filter: "all" | PackageSourceType;
  onFilterChange: (next: "all" | PackageSourceType) => void;
  search: string;
  onSearchChange: (next: string) => void;
  onAddClick: () => void;
}

export function PackageToolbar({
  filter,
  onFilterChange,
  search,
  onSearchChange,
  onAddClick,
}: PackageToolbarProps) {
  const { t } = useTranslation();
  return (
    <header className="flex flex-wrap items-center justify-between gap-3">
      <div
        className="flex flex-wrap items-center gap-2"
        role="tablist"
        aria-label={t("packages.title")}
      >
        {FILTER_ORDER.map((value) => (
          <Button
            key={value}
            type="button"
            size="sm"
            variant={filter === value ? "default" : "outline"}
            onClick={() => onFilterChange(value)}
            role="tab"
            aria-selected={filter === value}
            data-testid={`packages-filter-${value}`}
          >
            {t(`packages.filter.${value}`)}
          </Button>
        ))}
      </div>
      <div className="flex items-center gap-2">
        <Input
          value={search}
          onChange={(e) => onSearchChange(e.target.value)}
          placeholder={t("packages.search")}
          data-testid="packages-search"
          className="w-56"
        />
        <Button type="button" onClick={onAddClick} data-testid="packages-add-trigger">
          <Plus className="mr-1 h-4 w-4" aria-hidden />
          {t("packages.actions.add")}
        </Button>
      </div>
    </header>
  );
}
