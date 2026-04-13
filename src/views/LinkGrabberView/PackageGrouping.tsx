import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useTranslation } from "react-i18next";
import type { GroupingMode } from "./types";

interface PackageGroupingProps {
  mode: GroupingMode;
  onModeChange: (m: GroupingMode) => void;
}

export function PackageGrouping({ mode, onModeChange }: PackageGroupingProps) {
  const { t } = useTranslation();

  return (
    <div className="flex items-center gap-2">
      <label id="grouping-label" className="text-sm font-semibold">
        {t("linkGrabber.grouping.label")}
      </label>
      <Select value={mode} onValueChange={(v) => onModeChange(v as GroupingMode)}>
        <SelectTrigger className="w-40" aria-labelledby="grouping-label">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="none">{t("linkGrabber.grouping.none")}</SelectItem>
          <SelectItem value="hostname">{t("linkGrabber.grouping.hostname")}</SelectItem>
          <SelectItem value="extension">{t("linkGrabber.grouping.extension")}</SelectItem>
          <SelectItem value="type">{t("linkGrabber.grouping.type")}</SelectItem>
        </SelectContent>
      </Select>
    </div>
  );
}
