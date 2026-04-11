import { useState } from "react";
import { Switch } from "@/components/ui/switch";
import { useTauriMutation } from "@/api/hooks";
import { tauriInvoke } from "@/api/client";
import { useSettingsStore } from "@/stores/settingsStore";
import { PasteZone } from "./PasteZone";
import { FilterBar } from "./FilterBar";
import { PackageGrouping } from "./PackageGrouping";
import { ActionsBar } from "./ActionsBar";
import { ResolvedLinksSection } from "./ResolvedLinksSection";
import type { ResolvedLink, FilterType, GroupingMode } from "./types";

export function LinkGrabberView() {
  const [resolvedLinks, setResolvedLinks] = useState<ResolvedLink[]>([]);
  const [filter, setFilter] = useState<FilterType>("all");
  const [selectedLinkIds, setSelectedLinkIds] = useState<string[]>([]);
  const [groupingMode, setGroupingMode] = useState<GroupingMode>("hostname");

  const clipboardMonitoringEnabled = useSettingsStore(
    (s) => s.config?.clipboardMonitoring ?? false,
  );

  const { mutate: resolveLinks, isPending: isResolving } = useTauriMutation<
    ResolvedLink[],
    { urls: string[] }
  >("link_resolve", {
    onSuccess: (resolved) => {
      setResolvedLinks(resolved);
      setSelectedLinkIds([]);
    },
  });

  const { mutate: startDownload } = useTauriMutation<unknown, { url: string }>(
    "download_start",
  );

  const handlePasteUrls = (urls: string[]) => {
    const validUrls = urls.filter(
      (u) =>
        u.startsWith("http://") ||
        u.startsWith("https://") ||
        u.startsWith("ftp://") ||
        u.startsWith("magnet:") ||
        u.startsWith("container:"),
    );
    if (validUrls.length > 0) {
      resolveLinks({ urls: validUrls });
    }
  };

  const handleStartSelected = () => {
    for (const id of selectedLinkIds) {
      const link = resolvedLinks.find((l) => l.id === id);
      if (link?.resolvedUrl) {
        startDownload({ url: link.resolvedUrl });
      }
    }
  };

  const handleStartAllOnline = () => {
    for (const link of resolvedLinks) {
      if (link.status === "online" && link.resolvedUrl) {
        startDownload({ url: link.resolvedUrl });
      }
    }
  };

  return (
    <div className="flex h-full flex-col gap-4 p-4">
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-bold">Link Grabber</h1>
        <div className="flex items-center gap-2" title="Coming in a future update">
          <label className="text-sm" htmlFor="clipboard-toggle">
            Clipboard Monitoring
          </label>
          <Switch
            id="clipboard-toggle"
            checked={clipboardMonitoringEnabled}
            disabled
            onCheckedChange={async (enabled) => {
              await tauriInvoke("command_toggle_clipboard_monitoring", {
                enabled,
              });
            }}
          />
        </div>
      </div>

      <PasteZone onPasteUrls={handlePasteUrls} isLoading={isResolving} />

      {resolvedLinks.length > 0 && (
        <>
          <FilterBar activeFilter={filter} onFilterChange={setFilter} />
          <PackageGrouping mode={groupingMode} onModeChange={setGroupingMode} />
          <ActionsBar
            selectedCount={selectedLinkIds.length}
            totalCount={resolvedLinks.length}
            onStartSelected={handleStartSelected}
            onStartAll={handleStartAllOnline}
            onClearAll={() => {
              setResolvedLinks([]);
              setSelectedLinkIds([]);
            }}
            onSelectAll={() =>
              setSelectedLinkIds(resolvedLinks.map((l) => l.id))
            }
          />
          <ResolvedLinksSection
            links={resolvedLinks}
            filter={filter}
            groupingMode={groupingMode}
            selectedIds={selectedLinkIds}
            onSelectIds={setSelectedLinkIds}
          />
        </>
      )}
    </div>
  );
}
