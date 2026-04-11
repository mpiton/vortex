import { Button } from "@/components/ui/button";
import type { FilterType } from "./types";

interface FilterBarProps {
  activeFilter: FilterType;
  onFilterChange: (f: FilterType) => void;
}

export function FilterBar({ activeFilter, onFilterChange }: FilterBarProps) {
  const filters: { type: FilterType; label: string }[] = [
    { type: "all", label: "All" },
    { type: "online", label: "Online" },
    { type: "offline", label: "Offline" },
    { type: "media", label: "Media" },
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
