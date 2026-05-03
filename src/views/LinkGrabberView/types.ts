/**
 * Frontend status union accepted by `LinkRow`. Combines:
 *  - the legacy values produced by `link_resolve` (`checking`, `online`,
 *    `offline`, `error`)
 *  - the new values produced by `link_check_online` via the
 *    `link-status-updated` event (`premiumOnly`, `unknown`)
 *
 * `error` is treated as a synonym of `unknown` (both surface a generic
 * "could not be determined" badge).
 */
export type LinkStatus = "checking" | "online" | "offline" | "error" | "premiumOnly" | "unknown";

export interface ResolvedLink {
  id: string;
  originalUrl: string;
  resolvedUrl: string | null;
  filename: string | null;
  sizeBytes: number | null;
  status: LinkStatus;
  errorMessage?: string;
  moduleName: string;
  isMedia: boolean;
  mediaType?: "video" | "audio";
}

export type FilterType = "all" | "online" | "offline" | "media";

export type GroupingMode = "none" | "hostname" | "extension" | "type";
