import type { SegmentView } from "@/types/download";
import { formatBytes } from "@/lib/format";

const SEGMENT_COLORS = [
  "bg-blue-500",
  "bg-purple-500",
  "bg-pink-500",
  "bg-red-500",
  "bg-orange-500",
  "bg-yellow-500",
  "bg-green-500",
  "bg-teal-500",
];

interface SegmentVisualizationProps {
  segments: SegmentView[];
  totalBytes: number | null;
}

function computeProgress(segment: SegmentView, totalBytes: number | null): number {
  const segmentSize = segment.endByte - segment.startByte;
  if (segmentSize <= 0 || totalBytes === 0 || totalBytes === null) return 0;
  return Math.min(100, (segment.downloadedBytes / segmentSize) * 100);
}

export function SegmentVisualization({ segments, totalBytes }: SegmentVisualizationProps) {
  if (segments.length === 0) {
    return (
      <section className="space-y-3">
        <h3 className="text-sm font-semibold">Segments</h3>
        <p className="text-xs text-muted-foreground">No segments</p>
      </section>
    );
  }

  return (
    <section className="space-y-3">
      <h3 className="text-sm font-semibold">Segments ({segments.length})</h3>
      <div className="flex flex-col gap-2">
        {segments.map((segment, index) => {
          const color = SEGMENT_COLORS[index % SEGMENT_COLORS.length];
          const progress = computeProgress(segment, totalBytes);
          const segmentSize = segment.endByte - segment.startByte;

          return (
            <div key={segment.id} className="flex flex-col gap-1">
              <div className="flex items-center justify-between text-xs text-muted-foreground">
                <span>Segment {index + 1}</span>
                <span className="flex gap-2">
                  <span>{segment.state}</span>
                  <span>
                    {formatBytes(segment.downloadedBytes)} / {formatBytes(segmentSize)}
                  </span>
                  <span>{progress.toFixed(1)}%</span>
                </span>
              </div>
              <div className="h-2 w-full rounded-full bg-muted overflow-hidden">
                <div
                  className={`h-full rounded-full transition-all ${color}`}
                  style={{ width: `${progress}%` }}
                />
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}
