import { describe, it, expect } from "vitest";
import { canonicalPlaylistKey } from "../canonicalPlaylistKey";

describe("canonicalPlaylistKey", () => {
  it("collapses YouTube watch+list URLs to the canonical playlist token", () => {
    expect(
      canonicalPlaylistKey("https://www.youtube.com/watch?v=xyz&list=PL12345"),
    ).toBe("youtube:playlist:PL12345");
  });

  it("collapses YouTube playlist?list URLs to the same canonical token", () => {
    expect(
      canonicalPlaylistKey("https://www.youtube.com/playlist?list=PL12345"),
    ).toBe("youtube:playlist:PL12345");
  });

  it("normalises across host variants for the same playlist id", () => {
    const a = canonicalPlaylistKey("https://m.youtube.com/playlist?list=PL12345");
    const b = canonicalPlaylistKey(
      "https://music.youtube.com/watch?v=other&list=PL12345",
    );
    const c = canonicalPlaylistKey("https://youtube.com/playlist?list=PL12345");
    expect(a).toBe("youtube:playlist:PL12345");
    expect(b).toBe("youtube:playlist:PL12345");
    expect(c).toBe("youtube:playlist:PL12345");
  });

  it("falls back to the raw URL when no list query parameter is present", () => {
    const watchUrl = "https://www.youtube.com/watch?v=xyz";
    expect(canonicalPlaylistKey(watchUrl)).toBe(watchUrl);
  });

  it("collapses SoundCloud playlist URLs to the canonical path token", () => {
    expect(
      canonicalPlaylistKey("https://soundcloud.com/forss/sets/holiday-mix"),
    ).toBe("soundcloud:/forss/sets/holiday-mix");
  });

  it("normalises SoundCloud host variants and tracking params to the same key", () => {
    const a = canonicalPlaylistKey(
      "https://soundcloud.com/forss/sets/holiday-mix",
    );
    const b = canonicalPlaylistKey(
      "https://m.soundcloud.com/forss/sets/holiday-mix",
    );
    const c = canonicalPlaylistKey(
      "https://www.soundcloud.com/forss/sets/holiday-mix?in=somebody/sets/playlist&utm_source=mobile",
    );
    const d = canonicalPlaylistKey(
      "https://soundcloud.com/forss/sets/holiday-mix/",
    );
    expect(a).toBe("soundcloud:/forss/sets/holiday-mix");
    expect(b).toBe(a);
    expect(c).toBe(a);
    expect(d).toBe(a);
  });

  it("lowercases SoundCloud paths so case-only differences collapse", () => {
    expect(
      canonicalPlaylistKey("https://soundcloud.com/Forss/Sets/Holiday-Mix"),
    ).toBe("soundcloud:/forss/sets/holiday-mix");
  });

  it("returns the raw URL for unrecognised hosts (no canonical scheme yet)", () => {
    const otherUrl = "https://vimeo.com/album/abc123";
    expect(canonicalPlaylistKey(otherUrl)).toBe(otherUrl);
  });

  it("leaves SoundCloud short-share hosts untouched (cannot resolve without HTTP)", () => {
    const shortUrl = "https://on.soundcloud.com/abc123";
    expect(canonicalPlaylistKey(shortUrl)).toBe(shortUrl);
  });

  it("collapses youtu.be short-share URLs to the same canonical token", () => {
    expect(canonicalPlaylistKey("https://youtu.be/abc123?list=PL12345")).toBe(
      "youtube:playlist:PL12345",
    );
    // Equivalence with the long-form URL.
    expect(canonicalPlaylistKey("https://youtu.be/xyz?list=PL12345")).toBe(
      canonicalPlaylistKey("https://www.youtube.com/playlist?list=PL12345"),
    );
  });

  it("returns the raw input on malformed URLs without throwing", () => {
    expect(canonicalPlaylistKey("not a url")).toBe("not a url");
    expect(canonicalPlaylistKey("")).toBe("");
  });
});
