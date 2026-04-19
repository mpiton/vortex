import type { PluginStoreEntry } from "@/types/plugin-store";

export interface PluginGroup {
  category: string;
  entries: PluginStoreEntry[];
}

export function groupByCategory(entries: PluginStoreEntry[]): PluginGroup[] {
  const map = new Map<string, PluginStoreEntry[]>();
  for (const entry of entries) {
    const bucket = map.get(entry.category);
    if (bucket) {
      bucket.push(entry);
    } else {
      map.set(entry.category, [entry]);
    }
  }
  return Array.from(map, ([category, groupEntries]) => ({
    category,
    entries: groupEntries,
  }));
}
