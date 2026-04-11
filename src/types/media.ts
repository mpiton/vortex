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
  thumbnailUrl: string;
  durationSeconds: number;
  isPlaylist: boolean;
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
}
