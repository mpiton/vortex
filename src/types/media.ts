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
