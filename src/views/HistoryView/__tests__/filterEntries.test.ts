import { describe, it, expect } from "vitest";
import type { HistoryView } from "@/types/download";
import { deriveHistoryStatus, deriveHostname, filterHistoryEntries } from "../filterEntries";

function entry(overrides: Partial<HistoryView>): HistoryView {
  return {
    entryId: "1",
    downloadId: "1",
    fileName: "movie.mkv",
    url: "https://www.example.com/a/b/movie.mkv",
    totalBytes: 1024,
    completedAt: 1700000000,
    durationSeconds: 10,
    avgSpeed: 100,
    destinationPath: "/tmp/movie.mkv",
    ...overrides,
  };
}

describe("deriveHostname", () => {
  it("should extract hostname from valid URL", () => {
    expect(deriveHostname("https://files.example.com/a")).toBe("files.example.com");
  });

  it("should return em-dash for invalid URL", () => {
    expect(deriveHostname("not-a-url")).toBe("—");
  });
});

describe("deriveHistoryStatus", () => {
  it("should default to completed for all persisted entries", () => {
    expect(deriveHistoryStatus(entry({}))).toBe("completed");
  });
});

describe("filterHistoryEntries", () => {
  const entries: HistoryView[] = [
    entry({ entryId: "1", fileName: "alpha.zip", url: "https://a.example.com/alpha.zip" }),
    entry({ entryId: "2", fileName: "beta.mkv", url: "https://b.example.com/beta.mkv" }),
    entry({ entryId: "3", fileName: "gamma.pdf", url: "https://c.example.com/gamma.pdf" }),
  ];

  it("should return all entries on filter=all with empty search", () => {
    const result = filterHistoryEntries(entries, { filter: "all", searchQuery: "" });
    expect(result).toHaveLength(3);
  });

  it("should return all entries on filter=completed (all persisted = completed)", () => {
    const result = filterHistoryEntries(entries, { filter: "completed", searchQuery: "" });
    expect(result).toHaveLength(3);
  });

  it("should return none on filter=failed (backend does not yet persist failures)", () => {
    const result = filterHistoryEntries(entries, { filter: "failed", searchQuery: "" });
    expect(result).toHaveLength(0);
  });

  it("should return none on filter=cancelled", () => {
    const result = filterHistoryEntries(entries, { filter: "cancelled", searchQuery: "" });
    expect(result).toHaveLength(0);
  });

  it("should match search by file name case-insensitive", () => {
    const result = filterHistoryEntries(entries, { filter: "all", searchQuery: "BETA" });
    expect(result.map((e) => e.entryId)).toEqual(["2"]);
  });

  it("should match search by URL", () => {
    const result = filterHistoryEntries(entries, {
      filter: "all",
      searchQuery: "c.example.com",
    });
    expect(result.map((e) => e.entryId)).toEqual(["3"]);
  });

  it("should match search by hostname", () => {
    const result = filterHistoryEntries(entries, { filter: "all", searchQuery: "b.example" });
    expect(result.map((e) => e.entryId)).toEqual(["2"]);
  });

  it("should return all entries for whitespace-only search", () => {
    const result = filterHistoryEntries(entries, { filter: "all", searchQuery: "   " });
    expect(result).toHaveLength(3);
  });
});
