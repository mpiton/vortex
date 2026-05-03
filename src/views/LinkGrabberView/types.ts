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

/**
 * Where a duplicate of the URL was already found:
 *  - `active` — an entry already lives in the downloads list
 *  - `history` — an entry already lives in the completed-downloads history
 */
export type DuplicateSource = "active" | "history";

/**
 * Result of `link_detect_duplicates` for one URL. Mirrors
 * `DuplicateCheckDto` on the Rust side (camelCase IPC).
 */
export interface DuplicateCheck {
  url: string;
  isDuplicate: boolean;
  source: DuplicateSource | null;
  existingId: string | null;
  existingFilename: string | null;
}

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
  /**
   * Result of the duplicate-detection pass. `null` until the backend
   * has answered; `{ source: null, … }` once the probe has confirmed
   * the URL is unique.
   */
  duplicate?: DuplicateCheck | null;
}

export type FilterType = "all" | "online" | "offline" | "media";

export type GroupingMode = "none" | "hostname" | "extension" | "type";
