const YOUTUBE_HOSTS = new Set([
  "youtube.com",
  "www.youtube.com",
  "m.youtube.com",
  "music.youtube.com",
  // Short-share host for YouTube. Surfaced when users paste links from
  // the YouTube mobile share sheet (`https://youtu.be/<video>?list=PL…`).
  "youtu.be",
]);

/**
 * Reduce a media URL to a stable key for playlist grouping. The same
 * playlist accessed via `playlist?list=PL…` and `watch?v=…&list=PL…`
 * collapses to one key so the backend's `external_id` lookup actually
 * dedupes across URL variants. Sources without a canonical id (today
 * SoundCloud, where the URL path is already stable) keep their raw URL.
 */
export function canonicalPlaylistKey(url: string): string {
  try {
    const parsed = new URL(url);
    if (YOUTUBE_HOSTS.has(parsed.hostname)) {
      const list = parsed.searchParams.get("list");
      if (list) return `youtube:playlist:${list}`;
    }
  } catch {
    // Malformed URL — fall through to the raw value.
  }
  return url;
}
