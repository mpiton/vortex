import { Button } from "@/components/ui/button";
import { useTranslation } from "react-i18next";

interface ActionsBarProps {
  selectedCount: number;
  totalCount: number;
  onStartSelected: () => void;
  onStartAll: () => void;
  onClearAll: () => void;
  onSelectAll: () => void;
}

export function ActionsBar({
  selectedCount,
  totalCount,
  onStartSelected,
  onStartAll,
  onClearAll,
  onSelectAll,
}: ActionsBarProps) {
  const { t } = useTranslation();

  return (
    <div className="flex gap-2">
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
    </div>
  );
}
