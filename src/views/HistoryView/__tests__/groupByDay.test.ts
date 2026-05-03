import { describe, it, expect } from "vitest";
import type { HistoryView } from "@/types/download";
import { groupByDay } from "../groupByDay";

function entry(overrides: Partial<HistoryView>): HistoryView {
  return {
    entryId: "1",
    downloadId: "1",
    fileName: "file.zip",
    url: "https://example.com/file.zip",
    totalBytes: 1024,
    completedAt: 1700000000,
    durationSeconds: 10,
    avgSpeed: 100,
    destinationPath: "/tmp/file.zip",
    ...overrides,
  };
}

describe("groupByDay", () => {
  it("should return empty array when no entries", () => {
    expect(groupByDay([])).toEqual([]);
  });

  it("should group entries completed on the same local day", () => {
    const base = new Date(2026, 3, 20, 8, 0, 0).getTime() / 1000;
    const later = new Date(2026, 3, 20, 23, 30, 0).getTime() / 1000;
    const groups = groupByDay([
      entry({ entryId: "1", completedAt: base }),
      entry({ entryId: "2", completedAt: later }),
    ]);
    expect(groups).toHaveLength(1);
    expect(groups[0].entries).toHaveLength(2);
  });

  it("should split entries across local days", () => {
    const day1 = new Date(2026, 3, 20, 12, 0, 0).getTime() / 1000;
    const day2 = new Date(2026, 3, 21, 12, 0, 0).getTime() / 1000;
    const groups = groupByDay([
      entry({ entryId: "1", completedAt: day1 }),
      entry({ entryId: "2", completedAt: day2 }),
    ]);
    expect(groups).toHaveLength(2);
  });

  it("should sort groups by recency (newest first)", () => {
    const day1 = new Date(2026, 3, 10, 12, 0, 0).getTime() / 1000;
    const day2 = new Date(2026, 3, 20, 12, 0, 0).getTime() / 1000;
    const groups = groupByDay([
      entry({ entryId: "1", completedAt: day1 }),
      entry({ entryId: "2", completedAt: day2 }),
    ]);
    expect(groups[0].entries[0].entryId).toBe("2");
    expect(groups[1].entries[0].entryId).toBe("1");
  });

  it("should sort entries inside a group by recency", () => {
    const early = new Date(2026, 3, 20, 8, 0, 0).getTime() / 1000;
    const late = new Date(2026, 3, 20, 23, 30, 0).getTime() / 1000;
    const groups = groupByDay([
      entry({ entryId: "1", completedAt: early }),
      entry({ entryId: "2", completedAt: late }),
    ]);
    expect(groups[0].entries[0].entryId).toBe("2");
    expect(groups[0].entries[1].entryId).toBe("1");
  });

  it("should expose a YYYY-MM-DD day key", () => {
    const march21 = new Date(2026, 2, 21, 12, 0, 0).getTime() / 1000;
    const [group] = groupByDay([entry({ completedAt: march21 })]);
    expect(group.dayKey).toBe("2026-03-21");
  });
});
