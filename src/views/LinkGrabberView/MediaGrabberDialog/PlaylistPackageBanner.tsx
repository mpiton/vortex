import { useTranslation } from "react-i18next";
import { FolderTree } from "lucide-react";

interface PlaylistPackageBannerProps {
  /** Effective package name shown to the user. Falls back to a localized
   * "Playlist" label when the input is empty. */
  packageName: string;
  /** Number of items the resolved playlist exposes. */
  itemCount: number;
  /** When true, indicates the auto-grouper found an existing package
   * with the same `playlistId` and will reuse it (idempotent re-resolve). */
  willReuseExisting?: boolean;
}

export function PlaylistPackageBanner({
  packageName,
  itemCount,
  willReuseExisting,
}: PlaylistPackageBannerProps) {
  const { t } = useTranslation();
  const displayName = packageName.trim().length > 0
    ? packageName
    : t("mediaGrabber.playlistBanner.defaultName");

  const messageKey = willReuseExisting
    ? "mediaGrabber.playlistBanner.willReuse"
    : "mediaGrabber.playlistBanner.willCreate";

  return (
    <div
      role="status"
      data-testid="playlist-package-banner"
      className="flex items-center gap-3 rounded-md border border-primary/30 bg-primary/5 p-3 text-sm"
    >
      <FolderTree className="h-4 w-4 shrink-0 text-primary" aria-hidden="true" />
      <p className="leading-tight">
        {t(messageKey, {
          name: displayName,
          count: itemCount,
        })}
      </p>
    </div>
  );
}
