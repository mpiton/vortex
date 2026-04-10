import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { GroupingMode } from "./types";

interface PackageGroupingProps {
  mode: GroupingMode;
  onModeChange: (m: GroupingMode) => void;
}

export function PackageGrouping({ mode, onModeChange }: PackageGroupingProps) {
  return (
    <div className="flex items-center gap-2">
      <label className="text-sm font-semibold">Group Into Packages:</label>
      <Select value={mode} onValueChange={(v) => onModeChange(v as GroupingMode)}>
        <SelectTrigger className="w-40">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="none">No Grouping</SelectItem>
          <SelectItem value="hostname">By Hostname</SelectItem>
          <SelectItem value="extension">By Extension</SelectItem>
          <SelectItem value="type">By Type</SelectItem>
        </SelectContent>
      </Select>
    </div>
  );
}
