import type { AccountListFilter } from "@/types/account";
import type { DownloadFilter } from "@/types/download";
import type { PackageListFilter } from "@/types/package";

export const downloadQueries = {
  all: () => ["downloads"] as const,
  lists: () => [...downloadQueries.all(), "list"] as const,
  list: (filters?: DownloadFilter) =>
    filters
      ? ([...downloadQueries.lists(), filters] as const)
      : (downloadQueries.lists() as readonly unknown[]),
  details: () => [...downloadQueries.all(), "detail"] as const,
  detail: (id: string) => [...downloadQueries.details(), id] as const,
  countByState: () => [...downloadQueries.all(), "countByState"] as const,
};

export const pluginQueries = {
  all: () => ["plugins"] as const,
  lists: () => [...pluginQueries.all(), "list"] as const,
  list: () => pluginQueries.lists(),
};

export const historyQueries = {
  all: () => ["history"] as const,
  lists: () => [...historyQueries.all(), "list"] as const,
  list: () => historyQueries.lists(),
};

export const statsQueries = {
  all: () => ["stats"] as const,
  overview: () => [...statsQueries.all(), "overview"] as const,
};

export const packageQueries = {
  all: () => ["packages"] as const,
  lists: () => [...packageQueries.all(), "list"] as const,
  list: (filter?: PackageListFilter) =>
    filter
      ? ([...packageQueries.lists(), filter] as const)
      : (packageQueries.lists() as readonly unknown[]),
  details: () => [...packageQueries.all(), "detail"] as const,
  detail: (id: string) => [...packageQueries.details(), id] as const,
  downloads: (id: string) => [...packageQueries.all(), "downloads", id] as const,
};

export const accountQueries = {
  all: () => ["accounts"] as const,
  lists: () => [...accountQueries.all(), "list"] as const,
  list: (filter?: AccountListFilter) =>
    filter
      ? ([...accountQueries.lists(), filter] as const)
      : (accountQueries.lists() as readonly unknown[]),
  details: () => [...accountQueries.all(), "detail"] as const,
  detail: (id: string) => [...accountQueries.details(), id] as const,
  traffic: (id: string) => [...accountQueries.all(), "traffic", id] as const,
};
