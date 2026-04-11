import { Button } from "@/components/ui/button";

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
  return (
    <div className="flex gap-2">
      <Button onClick={onSelectAll} variant="outline" size="sm">
        Select All ({totalCount})
      </Button>
      {selectedCount > 0 && (
        <Button onClick={onStartSelected} size="sm">
          Start Selected ({selectedCount})
        </Button>
      )}
      <Button onClick={onStartAll} variant="secondary" size="sm">
        Start All Online
      </Button>
      <Button onClick={onClearAll} variant="destructive" size="sm">
        Clear
      </Button>
    </div>
  );
}
