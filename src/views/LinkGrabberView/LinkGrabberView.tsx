import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate } from "react-router";
import { Switch } from "@/components/ui/switch";
import { useTauriMutation } from "@/api/hooks";
import { toast } from "@/lib/toast";
import { useSettingsStore } from "@/stores/settingsStore";
import { useLinkGrabberStore } from "@/stores/linkGrabberStore";
import { useClipboardMonitoring } from "@/hooks/useClipboardMonitoring";
import { useLinkStatusEvents } from "@/hooks/useLinkStatusEvents";
import { PasteZone } from "./PasteZone";
import { FilterBar } from "./FilterBar";
import { PackageGrouping } from "./PackageGrouping";
import { ActionsBar } from "./ActionsBar";
import { ResolvedLinksSection } from "./ResolvedLinksSection";
import { MediaGrabberDialog } from "./MediaGrabberDialog";
import type { DuplicateCheck, ResolvedLink, FilterType, GroupingMode } from "./types";
import type {
  MediaDownloadResult,
  MediaGrabberOptions,
  PlaylistGroupInput,
  PlaylistGroupResult,
} from "@/types/media";
import { invoke } from "@tauri-apps/api/core";
import { canonicalPlaylistKey } from "./canonicalPlaylistKey";

export function LinkGrabberView() {
  const { t } = useTranslation();
  const location = useLocation();
  const navigate = useNavigate();
  const [resolvedLinks, setResolvedLinks] = useState<ResolvedLink[]>([]);
  const [filter, setFilter] = useState<FilterType>("all");
  const [selectedLinkIds, setSelectedLinkIds] = useState<string[]>([]);
  const [groupingMode, setGroupingMode] = useState<GroupingMode>("hostname");
  const [selectedMediaLink, setSelectedMediaLink] = useState<ResolvedLink | null>(null);
  const [mediaGrabberOpen, setMediaGrabberOpen] = useState(false);
  const [skipDuplicates, setSkipDuplicates] = useState(true);

  const initialClipboardEnabled = useSettingsStore((s) => s.config?.clipboardMonitoring ?? false);
  const { isEnabled: clipboardMonitoringEnabled, toggle: toggleClipboard } =
    useClipboardMonitoring(initialClipboardEnabled);

  // Subscribe once for the lifetime of this view so backend
  // `link-status-updated` events update the per-row badge and filters.
  useLinkStatusEvents();
  const resetLinkStatuses = useLinkGrabberStore((s) => s.reset);
  const setManyLinkStatuses = useLinkGrabberStore((s) => s.setManyStatuses);
  const liveStatuses = useLinkGrabberStore((s) => s.statuses);

  const { mutate: checkLinksOnline } = useTauriMutation<void, { urls: string[] }>(
    "link_check_online",
  );

  // Monotonic counter so a stale `link_detect_duplicates` response from
  // an earlier resolve cannot clobber the duplicate state of a newer
  // batch. Each resolve increments it; each per-call `onSuccess`
  // captures the value at dispatch time and bails when superseded.
  const detectBatchRef = useRef(0);
  const { mutate: detectDuplicates } = useTauriMutation<DuplicateCheck[], { urls: string[] }>(
    "link_detect_duplicates",
  );

  const dispatchDuplicateDetection = (urls: string[]) => {
    if (urls.length === 0) return;
    detectBatchRef.current += 1;
    const batchId = detectBatchRef.current;
    const inFlight = new Set(urls);
    detectDuplicates(
      { urls },
      {
        onSuccess: (checks) => {
          if (batchId !== detectBatchRef.current) return;
          if (checks.length === 0) return;
          const byUrl = new Map<string, DuplicateCheck>();
          for (const check of checks) {
            byUrl.set(check.url, check);
          }
          setResolvedLinks((prev) => {
            let changed = false;
            const next = prev.map((link) => {
              const probe = byUrl.get(link.originalUrl);
              if (!probe || link.duplicate === probe) return link;
              changed = true;
              return { ...link, duplicate: probe };
            });
            // Skip the state update when no link's duplicate field actually
            // moved — keeps downstream memos and effects from re-running
            // for an all-unique batch.
            return changed ? next : prev;
          });
        },
        onError: () => {
          // IPC failed → resolve every row in this batch to the
          // sentinel `null` so `isStartable` no longer treats them as
          // "still loading" and silently rejects them. We can't tell
          // whether they're duplicates, but blocking the entire bulk
          // start when the user explicitly hit Start is worse than
          // letting the download proceed without the dup check.
          if (batchId !== detectBatchRef.current) return;
          setResolvedLinks((prev) => {
            let changed = false;
            const next = prev.map((link) => {
              if (!inFlight.has(link.originalUrl)) return link;
              if (link.duplicate !== undefined) return link;
              changed = true;
              return { ...link, duplicate: null };
            });
            return changed ? next : prev;
          });
        },
      },
    );
  };

  const { mutate: resolveLinks, isPending: isResolving } = useTauriMutation<
    ResolvedLink[],
    { urls: string[] }
  >("link_resolve", {
    onSuccess: (resolved) => {
      setResolvedLinks(resolved);
      setSelectedLinkIds([]);
      // Reset the previous batch's live statuses so a stale "offline"
      // badge from an earlier paste does not bleed onto a new URL.
      resetLinkStatuses();
      const eligibleUrls = resolved
        .map((link) => link.originalUrl)
        .filter(
          (u) => u.toLowerCase().startsWith("http://") || u.toLowerCase().startsWith("https://"),
        );
      if (eligibleUrls.length > 0) {
        // Pre-seed every row with `checking` so the spinner appears
        // synchronously instead of waiting for the backend's first
        // event to land.
        setManyLinkStatuses(eligibleUrls.map((url) => [url, { kind: "checking" }] as const));
        checkLinksOnline(
          { urls: eligibleUrls },
          {
            // Without this, an IPC failure leaves every row stuck on the
            // optimistic `checking` spinner; downgrade to `unknown` so
            // the row clears and the retry button surfaces.
            onError: () => {
              setManyLinkStatuses(eligibleUrls.map((url) => [url, { kind: "unknown" }] as const));
            },
          },
        );
      }
      // Run duplicate detection over the full resolve batch (including
      // ftp:// / magnet: rows — duplicate detection is purely lexical
      // and does not require an HTTP probe).
      dispatchDuplicateDetection(resolved.map((link) => link.originalUrl));
      toast.success(t("linkGrabber.toast.resolveSuccess", { count: resolved.length }));
    },
  });

  const { mutate: startDownload } = useTauriMutation<unknown, { url: string }>("download_start");

  const { mutateAsync: startMediaDownloadAsync } = useTauriMutation<
    MediaDownloadResult,
    {
      url: string;
      quality: string;
      format: string;
      audioOnly: boolean;
      title?: string;
      playlistItems: string[];
    }
  >("download_media_start");

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

  // The bulk-start helpers gate every row through this predicate. Without
  // the `online` check, rows whose live probe is offline / premiumOnly /
  // unknown would still trigger `download_start` and burn IPC calls. The
  // duplicate gate is opt-out via the `Skip duplicates` checkbox so
  // power users can force-redownload.
  const isStartable = (link: ResolvedLink) => {
    const effectiveStatus = liveStatuses[link.originalUrl]?.kind ?? link.status;
    if (effectiveStatus !== "online") return false;
    // While `Skip duplicates` is on, also block rows whose duplicate
    // probe hasn't returned yet. The backend always emits a
    // `DuplicateCheck` per input URL, so `link.duplicate === undefined`
    // means the IPC roundtrip is still in flight — letting the row
    // through here would defeat the safety toggle when the user hits
    // Start the moment paste/resolve completes.
    if (skipDuplicates) {
      if (link.duplicate === undefined) return false;
      if (link.duplicate?.isDuplicate) return false;
    }
    return true;
  };

  const startLink = (link: ResolvedLink) => {
    // `originalUrl` fallback: a row that flipped to online via
    // `link_check_online` after resolve had no `resolvedUrl`; using the
    // original lets the bulk action honour its own "online" badge.
    const url = link.resolvedUrl ?? link.originalUrl;
    if (url) startDownload({ url });
  };

  const handleStartSelected = () => {
    for (const id of selectedLinkIds) {
      const link = resolvedLinks.find((l) => l.id === id);
      if (link && isStartable(link)) startLink(link);
    }
  };

  const handleStartAllOnline = () => {
    for (const link of resolvedLinks) {
      if (isStartable(link)) startLink(link);
    }
  };

  const duplicateCount = useMemo(
    () => resolvedLinks.reduce((n, link) => (link.duplicate?.isDuplicate ? n + 1 : n), 0),
    [resolvedLinks],
  );

  const handleMediaClick = (link: ResolvedLink) => {
    setSelectedMediaLink(link);
    setMediaGrabberOpen(true);
  };

  const handleMediaGrabberConfirm = async (options: MediaGrabberOptions) => {
    if (!selectedMediaLink?.originalUrl) return;

    const url = selectedMediaLink.originalUrl;
    // Gate auto-grouping on the metadata-derived `isPlaylist` flag rather
    // than on `playlistItems.length`. The selection list is empty by
    // default — the backend interprets that as "download every track" —
    // so a `> 0` check would skip grouping for the most common path.
    const isPlaylistDownload = options.isPlaylist === true || options.playlistItems.length > 0;

    // Step 1 — start the downloads first. Creating / reusing the package
    // before this would leave an empty package behind on every failed
    // start (network, plugin, backend), accumulating clutter in the UI.
    let result: MediaDownloadResult;
    try {
      result = await startMediaDownloadAsync({
        url,
        quality: options.quality,
        format: options.format,
        audioOnly: options.audioOnly,
        title: options.title,
        playlistItems: options.playlistItems,
      });
    } catch {
      // `useTauriMutation` already surfaces a default error toast on
      // rejection; emitting one here would double-report the same
      // failure. Just bail out so we skip the success path.
      return;
    }

    // Step 2 — only now create / reuse the playlist package. A grouping
    // failure is non-fatal: the downloads themselves are running, the
    // package just won't auto-collect them.
    let packageId: string | undefined;
    if (isPlaylistDownload && result.downloadIds.length > 0) {
      try {
        const groupItemCount =
          options.playlistItems.length > 0
            ? options.playlistItems.length
            : (options.playlistItemCount ?? 0);
        // Prefer the canonical playlist key (e.g. `youtube:playlist:PLxxx`)
        // so equivalent URLs (`watch?v=…&list=…` vs `playlist?list=…`)
        // dedupe to the same package. Falls back to the raw URL when the
        // source has no canonical scheme yet (SoundCloud paths are already
        // stable, unknown sources keep the URL as the natural key).
        const playlistKey = canonicalPlaylistKey(url);
        const grouped = await invoke<PlaylistGroupResult[]>("link_group_playlists", {
          groups: [
            {
              playlistId: playlistKey,
              playlistName: options.title ?? "",
              itemCount: groupItemCount,
            } satisfies PlaylistGroupInput,
          ],
        });
        packageId = grouped[0]?.packageId;
      } catch (err) {
        // Non-fatal: downloads already run; the user can retry by
        // re-resolving the playlist later.
        toast.error(t("linkGrabber.toast.playlistGroupingFailed", { defaultValue: String(err) }));
      }
    }

    // Step 3 — attach the newly-created downloads to the auto-package.
    // Failures here are non-fatal but must not be silent: surface a
    // single toast when any attachment rejects so the user can retry.
    if (packageId && result.downloadIds.length > 0) {
      const attachOutcomes = await Promise.allSettled(
        result.downloadIds.map((downloadId) =>
          invoke<void>("package_add_download", {
            packageId,
            downloadId,
          }),
        ),
      );
      const failedAttachCount = attachOutcomes.filter((o) => o.status === "rejected").length;
      if (failedAttachCount > 0) {
        toast.error(
          t("linkGrabber.toast.playlistAttachFailed", {
            count: failedAttachCount,
            defaultValue: `${failedAttachCount} downloads could not be attached to the playlist package`,
          }),
        );
      }
    }

    toast.success(t("linkGrabber.toast.downloadStarted"));
    void navigate("/");
  };

  const pasteContent =
    location.state &&
    typeof location.state === "object" &&
    "pasteContent" in location.state &&
    typeof (location.state as { pasteContent?: unknown }).pasteContent === "string"
      ? (location.state as { pasteContent: string }).pasteContent
      : undefined;

  const pasteToken =
    location.state &&
    typeof location.state === "object" &&
    "pasteToken" in location.state &&
    typeof (location.state as { pasteToken?: unknown }).pasteToken === "string"
      ? (location.state as { pasteToken: string }).pasteToken
      : undefined;

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
        initialValue={pasteContent}
        initialValueToken={pasteToken}
      />

      {resolvedLinks.length > 0 && (
        <>
          <FilterBar activeFilter={filter} onFilterChange={setFilter} />
          <PackageGrouping mode={groupingMode} onModeChange={setGroupingMode} />
          <ActionsBar
            selectedCount={selectedLinkIds.length}
            totalCount={resolvedLinks.length}
            duplicateCount={duplicateCount}
            skipDuplicates={skipDuplicates}
            onSkipDuplicatesChange={setSkipDuplicates}
            onStartSelected={handleStartSelected}
            onStartAll={handleStartAllOnline}
            onClearAll={() => {
              setResolvedLinks([]);
              setSelectedLinkIds([]);
              resetLinkStatuses();
            }}
            onSelectAll={() => setSelectedLinkIds(resolvedLinks.map((l) => l.id))}
          />
          <ResolvedLinksSection
            links={resolvedLinks}
            filter={filter}
            groupingMode={groupingMode}
            selectedIds={selectedLinkIds}
            onSelectIds={setSelectedLinkIds}
            onMediaClick={handleMediaClick}
            onRetry={(url) => {
              // Optimistically flip the row back to "checking" so the
              // spinner returns immediately; the backend will replace
              // the status when its probe lands.
              setManyLinkStatuses([[url, { kind: "checking" }]]);
              checkLinksOnline(
                { urls: [url] },
                {
                  onError: () => {
                    setManyLinkStatuses([[url, { kind: "unknown" }]]);
                  },
                },
              );
            }}
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
