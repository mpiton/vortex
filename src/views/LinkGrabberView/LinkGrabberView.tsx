import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate } from "react-router";
import { Switch } from "@/components/ui/switch";
import { useTauriMutation } from "@/api/hooks";
import { toast } from "@/lib/toast";
import { useSettingsStore } from "@/stores/settingsStore";
import { useClipboardMonitoring } from "@/hooks/useClipboardMonitoring";
import { PasteZone } from "./PasteZone";
import { FilterBar } from "./FilterBar";
import { PackageGrouping } from "./PackageGrouping";
import { ActionsBar } from "./ActionsBar";
import { ResolvedLinksSection } from "./ResolvedLinksSection";
import { MediaGrabberDialog } from "./MediaGrabberDialog";
import type { ResolvedLink, FilterType, GroupingMode } from "./types";
import type { MediaDownloadResult, MediaGrabberOptions } from "@/types/media";

export function LinkGrabberView() {
  const { t } = useTranslation();
  const location = useLocation();
  const navigate = useNavigate();
  const [resolvedLinks, setResolvedLinks] = useState<ResolvedLink[]>([]);
  const [filter, setFilter] = useState<FilterType>("all");
  const [selectedLinkIds, setSelectedLinkIds] = useState<string[]>([]);
  const [groupingMode, setGroupingMode] = useState<GroupingMode>("hostname");
  const [selectedMediaLink, setSelectedMediaLink] =
    useState<ResolvedLink | null>(null);
  const [mediaGrabberOpen, setMediaGrabberOpen] = useState(false);

  const initialClipboardEnabled = useSettingsStore(
    (s) => s.config?.clipboardMonitoring ?? false,
  );
  const { isEnabled: clipboardMonitoringEnabled, toggle: toggleClipboard } =
    useClipboardMonitoring(initialClipboardEnabled);

  const { mutate: resolveLinks, isPending: isResolving } = useTauriMutation<
    ResolvedLink[],
    { urls: string[] }
  >("link_resolve", {
    onSuccess: (resolved) => {
      setResolvedLinks(resolved);
      setSelectedLinkIds([]);
      toast.success(t("linkGrabber.toast.resolveSuccess", { count: resolved.length }));
    },
  });

  const { mutate: startDownload } = useTauriMutation<unknown, { url: string }>(
    "download_start",
  );

  const { mutate: startMediaDownload } = useTauriMutation<
    MediaDownloadResult,
    {
      url: string;
      quality: string;
      format: string;
      audioOnly: boolean;
      title?: string;
      playlistItems: string[];
    }
  >("download_media_start", {
    onSuccess: () => {
      toast.success(t("linkGrabber.toast.downloadStarted"));
      void navigate("/");
    },
  });

  const handlePasteUrls = (urls: string[]) => {
    // TODO: container: entries need a dedicated backend command for decryption
    const validUrls = urls.filter(
      (u) =>
        u.startsWith("http://") ||
        u.startsWith("https://") ||
        u.startsWith("ftp://") ||
        u.startsWith("magnet:"),
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

  const handleMediaClick = (link: ResolvedLink) => {
    setSelectedMediaLink(link);
    setMediaGrabberOpen(true);
  };

  const handleMediaGrabberConfirm = (options: MediaGrabberOptions) => {
    if (selectedMediaLink?.originalUrl) {
      startMediaDownload({
        url: selectedMediaLink.originalUrl,
        quality: options.quality,
        format: options.format,
        audioOnly: options.audioOnly,
        title: options.title,
        playlistItems: options.playlistItems,
      });
    }
  };

  useEffect(() => {
    const shouldFocusPaste =
      !!location.state &&
      typeof location.state === "object" &&
      "focusPaste" in location.state &&
      location.state.focusPaste === true;

    if (!shouldFocusPaste) return;

    const textarea = document.querySelector<HTMLTextAreaElement>(
      '[data-shortcut-target="link-grabber-paste"]',
    );
    textarea?.focus();
    void navigate(location.pathname, { replace: true, state: null });
  }, [location.pathname, location.state, navigate]);

  return (
    <div className="flex h-full flex-col gap-4 p-4">
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-bold">{t("nav.linkGrabber")}</h1>
        <div
          className="flex items-center gap-2"
          title={
            clipboardMonitoringEnabled
              ? t("statusBar.clipboardActive")
              : t("statusBar.clipboardPaused")
          }
        >
          <label className="text-sm" htmlFor="clipboard-toggle">
            {t("linkGrabber.clipboardMonitoringLabel")}
          </label>
          <span
            aria-hidden="true"
            data-testid="clipboard-status-dot"
            className={`h-[7px] w-[7px] rounded-full transition-colors ${
              clipboardMonitoringEnabled ? "bg-success" : "bg-border"
            }`}
          />
          <Switch
            id="clipboard-toggle"
            checked={clipboardMonitoringEnabled}
            onCheckedChange={toggleClipboard}
          />
        </div>
      </div>

      <PasteZone
        onPasteUrls={handlePasteUrls}
        isLoading={isResolving}
      />

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
            onMediaClick={handleMediaClick}
          />
        </>
      )}
      {selectedMediaLink && (
        <MediaGrabberDialog
          link={selectedMediaLink}
          open={mediaGrabberOpen}
          onOpenChange={(open) => {
            setMediaGrabberOpen(open);
            if (!open) setSelectedMediaLink(null);
          }}
          onConfirm={handleMediaGrabberConfirm}
        />
      )}
    </div>
  );
}
