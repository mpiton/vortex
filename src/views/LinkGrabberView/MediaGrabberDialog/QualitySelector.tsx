import { Card } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import type { QualityOption } from "@/types/media";

interface QualitySelectorProps {
  qualities: QualityOption[];
  formats: string[];
  selected: string;
  selectedFormat: string;
  onSelectQuality: (quality: string) => void;
  onSelectFormat: (format: string) => void;
}

export function QualitySelector({
  qualities,
  formats,
  selected,
  selectedFormat,
  onSelectQuality,
  onSelectFormat,
}: QualitySelectorProps) {
  return (
    <section className="space-y-3">
      <h3 className="text-sm font-semibold">Video Quality</h3>

      <div className="grid grid-cols-3 gap-2" role="radiogroup" aria-label="Video quality">
        {qualities.map((q) => (
          <Card
            key={q.quality}
            role="radio"
            aria-checked={selected === q.quality}
            tabIndex={selected === q.quality ? 0 : -1}
            className={`cursor-pointer p-3 transition-colors ${
              selected === q.quality ? "ring-2 ring-accent bg-accent/10" : "hover:bg-muted"
            }`}
            onClick={() => onSelectQuality(q.quality)}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onSelectQuality(q.quality);
              }
            }}
          >
            <div className="text-sm font-semibold">{q.quality}</div>
            <div className="text-xs text-muted-foreground">
              {q.width}&times;{q.height} @ {q.fps}fps
            </div>
            <div className="text-xs text-muted-foreground">
              {(q.bitrateKbps / 1000).toFixed(1)} Mbps
            </div>
          </Card>
        ))}
      </div>

      <div className="space-y-2">
        <span className="text-sm font-semibold" id="container-format-label">
          Container Format
        </span>
        <div className="flex gap-2" role="group" aria-labelledby="container-format-label">
          {formats.map((fmt) => (
            <Button
              key={fmt}
              variant={selectedFormat === fmt ? "default" : "outline"}
              size="sm"
              onClick={() => onSelectFormat(fmt)}
              aria-pressed={selectedFormat === fmt}
              className="uppercase"
            >
              {fmt}
            </Button>
          ))}
        </div>
      </div>
    </section>
  );
}
