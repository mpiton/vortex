import { formatBytes } from "@/lib/format";

interface SizeEstimateProps {
  quality: string;
  format: string;
  duration: number;
}

const BITRATE_MAP: Record<string, number> = {
  "360p": 500,
  "480p": 1000,
  "720p": 2500,
  "1080p": 5000,
  "1440p": 8000,
  "4k": 15000,
  audio_only: 192,
};

export function SizeEstimate({ quality, format, duration }: SizeEstimateProps) {
  const bitrateKbps = BITRATE_MAP[quality] ?? 2500;
  const fileSizeBytes = ((bitrateKbps * 1000) * duration) / 8;

  return (
    <div className="rounded bg-muted p-3">
      <p className="text-sm font-semibold">
        Estimated Size: {formatBytes(fileSizeBytes)}
      </p>
      <p className="text-xs text-muted-foreground">
        {quality} {format.toUpperCase()} &bull; {Math.round(duration / 60)}m
        video
      </p>
    </div>
  );
}
