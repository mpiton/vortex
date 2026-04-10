export type LinkStatus = "checking" | "online" | "offline" | "error";

export interface ResolvedLink {
  id: string;
  originalUrl: string;
  resolvedUrl: string | null;
  filename: string | null;
  sizeBytes: number | null;
  status: LinkStatus;
  errorMessage?: string;
  moduleName: string;
  isMedia: boolean;
  mediaType?: "video" | "audio";
}

export type FilterType = "all" | "online" | "offline" | "media";

export type GroupingMode = "none" | "hostname" | "extension" | "type";
