export interface QualityOption {
  quality: string;
  height: number;
  width: number;
  fps: number;
  bitrateKbps: number;
}

export interface SubtitleLanguage {
  code: string;
  name: string;
}

export interface PlaylistItem {
  id: string;
  title: string;
  durationSeconds: number;
}

export interface MediaMetadata {
  title: string;
  artist?: string;
  thumbnailUrl: string;
  durationSeconds: number;
  isPlaylist: boolean;
  defaultQuality?: string;
  availableQualities: QualityOption[];
  availableFormats: string[];
  availableAudioFormats: string[];
  availableSubtitles: SubtitleLanguage[];
  playlistItems?: PlaylistItem[];
}

export interface MediaDownloadResult {
  downloadIds: number[];
}

export interface MediaGrabberOptions {
  quality: string;
  format: string;
  subtitles: string[];
  audioOnly: boolean;
  playlistItems: string[];
  /** True when the resolved media is a playlist. Independent from
   * `playlistItems`: the user may confirm without selecting any item, in
   * which case the backend downloads every track but the UI still needs
   * the playlist semantics for auto-grouping (PRD-v2 §P1.11). */
  isPlaylist?: boolean;
  /** Total number of items in the playlist as reported by the metadata
   * crawler. Used by the auto-grouping IPC for the `Will create package
   * X with N items` preview, independently from the user's selection. */
  playlistItemCount?: number;
  /** Video title used to build the filename (e.g. "Rick Astley - Never Gonna Give You Up").
   * The backend sanitises it and appends the format extension. */
  title?: string;
  /** When set, every download produced by this confirmation gets attached
   * to the given package (auto-grouping playlist → package, PRD-v2 §P1.11). */
  packageId?: string;
}

/** Mirror of [`PlaylistGroupInputDto`](src-tauri/src/adapters/driving/tauri_ipc.rs).
 * Used as the input payload to the `link_group_playlists` IPC. */
export interface PlaylistGroupInput {
  playlistId: string;
  playlistName: string;
  itemCount: number;
}

/** Mirror of [`PlaylistGroupResultDto`](src-tauri/src/adapters/driving/tauri_ipc.rs).
 * Returned by the `link_group_playlists` IPC. */
export interface PlaylistGroupResult {
  packageId: string;
  packageName: string;
  /** True when the package was just created, false when an existing
   * package with the same `playlistId` was reused. */
  created: boolean;
  itemCount: number;
}

/** Mirror of [`SplitArchiveLinkInputDto`](src-tauri/src/adapters/driving/tauri_ipc.rs).
 * Used as the input payload to the `link_group_split_archives` IPC. */
export interface SplitArchiveLinkInput {
  url: string;
  filename: string;
}

/** Mirror of [`SplitArchiveGroupResultDto`](src-tauri/src/adapters/driving/tauri_ipc.rs).
 * Returned by the `link_group_split_archives` IPC. One entry per detected
 * base name; `missingParts` is non-empty when the input batch had gaps in
 * the part numbering (the backend also fires a `split-archive-incomplete`
 * event in that case). */
export interface SplitArchiveGroupResult {
  packageId: string;
  baseName: string;
  packageName: string;
  /** True when the package was just created, false when an existing
   * package with the same `baseName` was reused. */
  created: boolean;
  /** URLs that belong to this group, sorted by detected part number. */
  urls: string[];
  /** Human-readable suffixes (e.g. `"part05.rar"`, `"7z.003"`) of the
   * parts that should exist between part 1 and the highest detected
   * part number but are absent from the input batch. */
  missingParts: string[];
}
