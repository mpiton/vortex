import { Checkbox } from "@/components/ui/checkbox";
import { LinkRow } from "./LinkRow";
import type { ResolvedLink, FilterType, GroupingMode } from "./types";

interface ResolvedLinksSectionProps {
  links: ResolvedLink[];
  filter: FilterType;
  groupingMode: GroupingMode;
  selectedIds: string[];
  onSelectIds: (ids: string[]) => void;
}

function getGroupKey(link: ResolvedLink, mode: GroupingMode): string {
  switch (mode) {
    case "hostname": {
      try {
        const hostname = new URL(link.originalUrl).hostname;
        return hostname || "Unknown";
      } catch {
        return "Unknown";
      }
    }
    case "extension": {
      const name = link.filename ?? "";
      const dotIndex = name.lastIndexOf(".");
      if (dotIndex <= 0) return "UNKNOWN";
      return name.slice(dotIndex + 1).toUpperCase() || "UNKNOWN";
    }
    case "type": {
      if (link.isMedia) return "Media";
      const ext = link.filename?.split(".").pop()?.toLowerCase() ?? "";
      if (["zip", "rar", "7z", "tar", "gz"].includes(ext)) return "Archive";
      return "Other";
    }
    default:
      return "Links";
  }
}

export function groupLinks(
  links: ResolvedLink[],
  mode: GroupingMode,
): Record<string, ResolvedLink[]> {
  if (mode === "none") {
    return { Links: links };
  }

  return links.reduce<Record<string, ResolvedLink[]>>((acc, link) => {
    const key = getGroupKey(link, mode);
    return {
      ...acc,
      [key]: [...(acc[key] ?? []), link],
    };
  }, {});
}

function applyFilter(links: ResolvedLink[], filter: FilterType): ResolvedLink[] {
  if (filter === "all") return links;
  if (filter === "online") return links.filter((l) => l.status === "online");
  if (filter === "offline") return links.filter((l) => l.status === "offline");
  return links.filter((l) => l.isMedia);
}

export function ResolvedLinksSection({
  links,
  filter,
  groupingMode,
  selectedIds,
  onSelectIds,
}: ResolvedLinksSectionProps) {
  const filtered = applyFilter(links, filter);
  const grouped = groupLinks(filtered, groupingMode);

  const handleGroupToggle = (linksInGroup: ResolvedLink[], checked: boolean) => {
    const groupIds = linksInGroup.map((l) => l.id);
    if (checked) {
      const next = Array.from(new Set([...selectedIds, ...groupIds]));
      onSelectIds(next);
    } else {
      onSelectIds(selectedIds.filter((id) => !groupIds.includes(id)));
    }
  };

  const handleLinkToggle = (linkId: string) => {
    if (selectedIds.includes(linkId)) {
      onSelectIds(selectedIds.filter((id) => id !== linkId));
    } else {
      onSelectIds([...selectedIds, linkId]);
    }
  };

  return (
    <div className="flex-1 overflow-y-auto rounded border">
      {Object.entries(grouped).map(([groupName, groupItems]) => {
        if (groupItems.length === 0) return null;
        const groupIds = groupItems.map((l) => l.id);
        const allSelected =
          groupIds.length > 0 && groupIds.every((id) => selectedIds.includes(id));
        return (
          <div key={groupName}>
            <div className="sticky top-0 flex items-center gap-2 bg-muted px-4 py-2">
              <Checkbox
                checked={allSelected}
                onCheckedChange={(checked) =>
                  handleGroupToggle(groupItems, !!checked)
                }
                aria-label={`Select all in ${groupName}`}
              />
              <span className="text-sm font-medium">{groupName}</span>
              <span className="text-xs text-muted-foreground">
                ({groupItems.length})
              </span>
            </div>
            <div className="space-y-1 p-2">
              {groupItems.map((link) => (
                <LinkRow
                  key={link.id}
                  link={link}
                  selected={selectedIds.includes(link.id)}
                  onSelect={() => handleLinkToggle(link.id)}
                />
              ))}
            </div>
          </div>
        );
      })}
    </div>
  );
}
