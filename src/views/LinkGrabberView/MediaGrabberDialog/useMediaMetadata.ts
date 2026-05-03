import { useTauriQuery } from "@/api/hooks";
import type { MediaMetadata } from "@/types/media";

export function useMediaMetadata(url: string, enabled: boolean) {
  return useTauriQuery<MediaMetadata>(
    "command_get_media_metadata",
    { url },
    {
      enabled,
      queryKey: ["command_get_media_metadata", { url }],
    },
  );
}
