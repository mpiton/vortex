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

  it("returns the raw URL for non-YouTube hosts (SoundCloud paths are already canonical)", () => {
    const scUrl = "https://soundcloud.com/forss/sets/holiday-mix";
    expect(canonicalPlaylistKey(scUrl)).toBe(scUrl);
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
