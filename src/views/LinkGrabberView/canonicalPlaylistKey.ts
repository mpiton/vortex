const YOUTUBE_HOSTS = new Set([
  "youtube.com",
  "www.youtube.com",
  "m.youtube.com",
  "music.youtube.com",
  // Short-share host for YouTube. Surfaced when users paste links from
  // the YouTube mobile share sheet (`https://youtu.be/<video>?list=PL…`).
  "youtu.be",
]);

const SOUNDCLOUD_HOSTS = new Set([
  "soundcloud.com",
  "www.soundcloud.com",
  "m.soundcloud.com",
]);

/**
 * Reduce a media URL to a stable key for playlist grouping. The same
 * playlist accessed via `playlist?list=PL…` and `watch?v=…&list=PL…`
 * collapses to one key so the backend's `external_id` lookup actually
 * dedupes across URL variants. Sources without a recognised canonical
 * scheme keep their raw URL as the natural key.
 *
 * - YouTube: collapses every host variant + `youtu.be` short shares
 *   to `youtube:playlist:<list-id>` keyed on the `list` query param.
 * - SoundCloud: collapses host variants + tracking parameters to
 *   `soundcloud:<lowercased-path>` keyed on the URL path (the
 *   user/set slug pair is the natural identifier of a playlist).
 *   Short-link hosts (`on.soundcloud.com/<id>`) cannot be resolved
 *   without an HTTP round-trip and stay as raw URLs.
 */
export function canonicalPlaylistKey(url: string): string {
  try {
    const parsed = new URL(url);
    if (YOUTUBE_HOSTS.has(parsed.hostname)) {
      const list = parsed.searchParams.get("list");
      if (list) return `youtube:playlist:${list}`;
    }
    if (SOUNDCLOUD_HOSTS.has(parsed.hostname)) {
      const path = parsed.pathname.replace(/\/+$/, "").toLowerCase();
      if (path) return `soundcloud:${path}`;
    }
  } catch {
    // Malformed URL — fall through to the raw value.
  }
  return url;
}
