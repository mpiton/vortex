import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { useTranslation } from "react-i18next";

interface ActionsBarProps {
  selectedCount: number;
  totalCount: number;
  duplicateCount: number;
  skipDuplicates: boolean;
  onSkipDuplicatesChange: (value: boolean) => void;
  onStartSelected: () => void;
  onStartAll: () => void;
  onClearAll: () => void;
  onSelectAll: () => void;
}

export function ActionsBar({
  selectedCount,
  totalCount,
  duplicateCount,
  skipDuplicates,
  onSkipDuplicatesChange,
  onStartSelected,
  onStartAll,
  onClearAll,
  onSelectAll,
}: ActionsBarProps) {
  const { t } = useTranslation();

  return (
    <div className="flex flex-wrap items-center gap-2">
      <Button onClick={onSelectAll} variant="outline" size="sm">
        {t("linkGrabber.actions.selectAll", { count: totalCount })}
      </Button>
      {selectedCount > 0 && (
        <Button onClick={onStartSelected} size="sm">
          {t("linkGrabber.actions.startSelected", { count: selectedCount })}
        </Button>
      )}
      <Button onClick={onStartAll} variant="secondary" size="sm">
        {t("linkGrabber.actions.startAllOnline")}
      </Button>
      <Button onClick={onClearAll} variant="destructive" size="sm">
        {t("common.clear")}
      </Button>
      <label className="ml-auto flex shrink-0 items-center gap-2 text-sm">
        <Checkbox
          id="link-grabber-skip-duplicates"
          checked={skipDuplicates}
          onCheckedChange={(checked) => onSkipDuplicatesChange(!!checked)}
          aria-label={t("linkGrabber.actions.skipDuplicates", {
            defaultValue: "Skip duplicates",
          })}
        />
        <span>
          {t("linkGrabber.actions.skipDuplicates", { defaultValue: "Skip duplicates" })}
          {duplicateCount > 0 && (
            <span className="ml-1 text-xs text-muted-foreground">({duplicateCount})</span>
          )}
        </span>
      </label>
    </div>
  );
}
