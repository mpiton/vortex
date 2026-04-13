import { Button } from "@/components/ui/button";
import { useTranslation } from "react-i18next";
import type { FilterType } from "./types";

interface FilterBarProps {
  activeFilter: FilterType;
  onFilterChange: (f: FilterType) => void;
}

export function FilterBar({ activeFilter, onFilterChange }: FilterBarProps) {
  const { t } = useTranslation();
  const filters: { type: FilterType; label: string }[] = [
    { type: "all", label: t("linkGrabber.filters.all") },
    { type: "online", label: t("linkGrabber.filters.online") },
    { type: "offline", label: t("linkGrabber.filters.offline") },
    { type: "media", label: t("linkGrabber.filters.media") },
  ];

  return (
    <div className="flex gap-2 border-b pb-2">
      {filters.map((f) => (
        <Button
          key={f.type}
          variant={activeFilter === f.type ? "default" : "outline"}
          size="sm"
          onClick={() => onFilterChange(f.type)}
        >
          {f.label}
        </Button>
      ))}
    </div>
  );
}
