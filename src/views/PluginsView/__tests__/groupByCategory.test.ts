import { describe, it, expect } from "vitest";
import { groupByCategory } from "../groupByCategory";
import type { PluginStoreEntry } from "@/types/plugin-store";

function entry(name: string, category: string): PluginStoreEntry {
  return {
    name,
    description: "",
    author: "",
    version: "1.0.0",
    installedVersion: null,
    category,
    official: true,
    status: "not_installed",
  };
}

describe("groupByCategory", () => {
  it("returns an empty array for an empty input", () => {
    expect(groupByCategory([])).toEqual([]);
  });

  it("groups entries by category preserving insertion order", () => {
    const input = [entry("a", "crawler"), entry("b", "hoster"), entry("c", "crawler")];
    const result = groupByCategory(input);
    expect(result).toHaveLength(2);
    expect(result[0].category).toBe("crawler");
    expect(result[0].entries.map((e) => e.name)).toEqual(["a", "c"]);
    expect(result[1].category).toBe("hoster");
    expect(result[1].entries.map((e) => e.name)).toEqual(["b"]);
  });

  it("keeps categories distinct even with unusual ordering", () => {
    const input = [
      entry("a", "captcha"),
      entry("b", "crawler"),
      entry("c", "captcha"),
      entry("d", "crawler"),
    ];
    const result = groupByCategory(input);
    expect(result.map((g) => g.category)).toEqual(["captcha", "crawler"]);
    expect(result[0].entries).toHaveLength(2);
    expect(result[1].entries).toHaveLength(2);
  });
});
