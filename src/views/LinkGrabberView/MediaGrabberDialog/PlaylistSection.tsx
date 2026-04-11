import { Checkbox } from "@/components/ui/checkbox";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { PlaylistItem } from "@/types/media";

interface PlaylistSectionProps {
  items: PlaylistItem[];
  selectedItems: string[];
  onSelectItems: (ids: string[]) => void;
}

export function PlaylistSection({
  items,
  selectedItems,
  onSelectItems,
}: PlaylistSectionProps) {
  const allSelected = items.length > 0 && selectedItems.length === items.length;

  return (
    <section className="space-y-3">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold">
          Playlist ({items.length} items)
        </h3>
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            if (allSelected) {
              onSelectItems([]);
            } else {
              onSelectItems(items.map((i) => i.id));
            }
          }}
        >
          {allSelected ? "Deselect All" : "Select All"}
        </Button>
      </div>

      <ScrollArea className="h-40 rounded border">
        <div className="space-y-1 p-2">
          {items.map((item, idx) => (
            <div
              key={item.id}
              className="flex items-center gap-2 rounded p-2 hover:bg-muted"
            >
              <Checkbox
                aria-label={`#${idx + 1} ${item.title}`}
                checked={selectedItems.includes(item.id)}
                onCheckedChange={(checked) => {
                  if (checked) {
                    onSelectItems([...selectedItems, item.id]);
                  } else {
                    onSelectItems(
                      selectedItems.filter((id) => id !== item.id),
                    );
                  }
                }}
              />
              <div className="min-w-0 flex-1">
                <span className="text-xs font-semibold text-muted-foreground">
                  #{idx + 1}
                </span>
                <p className="truncate text-sm">{item.title}</p>
                <p className="text-xs text-muted-foreground">
                  {Math.floor(item.durationSeconds / 60)}m{" "}
                  {item.durationSeconds % 60}s
                </p>
              </div>
            </div>
          ))}
        </div>
      </ScrollArea>
    </section>
  );
}
