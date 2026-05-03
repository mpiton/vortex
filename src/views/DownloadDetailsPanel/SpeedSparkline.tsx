import { useSpeedHistory } from "@/hooks/useSpeedHistory";
import { formatSpeed } from "@/lib/format";

const WIDTH = 300;
const HEIGHT = 60;
const PADDING = 4;

interface SpeedSparklineProps {
  downloadId: string;
}

export function SpeedSparkline({ downloadId }: SpeedSparklineProps) {
  const samples = useSpeedHistory(downloadId);

  if (samples.length < 2) {
    return (
      <section className="space-y-3">
        <h3 className="text-sm font-semibold">Speed History</h3>
        <p className="text-xs text-muted-foreground">No history yet</p>
      </section>
    );
  }

  const maxSpeed = Math.max(...samples.map((s) => s.speed), 1);
  const innerWidth = WIDTH - PADDING * 2;
  const innerHeight = HEIGHT - PADDING * 2;

  const points = samples.map((sample, i) => {
    const x = PADDING + (i / (samples.length - 1)) * innerWidth;
    const y = PADDING + (1 - sample.speed / maxSpeed) * innerHeight;
    return `${x},${y}`;
  });

  const polylinePoints = points.join(" ");

  return (
    <section className="space-y-3">
      <h3 className="text-sm font-semibold">Speed History (2 min)</h3>
      <svg width={WIDTH} height={HEIGHT} className="overflow-visible">
        {/* Bottom grid line */}
        <line
          x1={PADDING}
          y1={HEIGHT - PADDING}
          x2={WIDTH - PADDING}
          y2={HEIGHT - PADDING}
          stroke="hsl(var(--border))"
          strokeWidth={1}
        />
        {/* Top grid line */}
        <line
          x1={PADDING}
          y1={PADDING}
          x2={WIDTH - PADDING}
          y2={PADDING}
          stroke="hsl(var(--border))"
          strokeWidth={1}
        />
        {/* Speed polyline */}
        <polyline
          points={polylinePoints}
          fill="none"
          stroke="hsl(var(--primary))"
          strokeWidth={2}
          strokeLinejoin="round"
          strokeLinecap="round"
        />
      </svg>
      <p className="text-xs text-muted-foreground text-right">Max: {formatSpeed(maxSpeed)}</p>
    </section>
  );
}
