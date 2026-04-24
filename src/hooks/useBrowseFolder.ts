import { useCallback } from "react";
import { tauriInvoke } from "@/api/client";

export interface BrowseFileFilter {
  name: string;
  extensions: string[];
}

export interface BrowseFileOptions {
  filters?: BrowseFileFilter[];
  defaultPath?: string | null;
}

export function useBrowseFolder() {
  return useCallback(
    (defaultPath?: string | null): Promise<string | null> =>
      tauriInvoke<string | null>("browse_folder", {
        defaultPath: defaultPath ?? null,
      }),
    [],
  );
}

export function useBrowseFile() {
  return useCallback(
    (options?: BrowseFileOptions): Promise<string | null> =>
      tauriInvoke<string | null>("browse_file", {
        filters: options?.filters ?? null,
        defaultPath: options?.defaultPath ?? null,
      }),
    [],
  );
}
