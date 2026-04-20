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

export interface MediaGrabberOptions {
  quality: string;
  format: string;
  subtitles: string[];
  audioOnly: boolean;
  playlistItems: string[];
  /** Video title used to build the filename (e.g. "Rick Astley - Never Gonna Give You Up").
   * The backend sanitises it and appends the format extension. */
  title?: string;
}
