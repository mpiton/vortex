import { Switch } from "@/components/ui/switch";
import { Button } from "@/components/ui/button";

interface AudioOnlySectionProps {
  enabled: boolean;
  onEnabledChange: (enabled: boolean) => void;
  audioFormats: string[];
  selectedFormat: string;
  onSelectFormat: (format: string) => void;
}

export function AudioOnlySection({
  enabled,
  onEnabledChange,
  audioFormats,
  selectedFormat,
  onSelectFormat,
}: AudioOnlySectionProps) {
  return (
    <section className="space-y-3">
      <div className="flex items-center gap-2">
        <Switch
          id="audio-only"
          checked={enabled}
          onCheckedChange={onEnabledChange}
        />
        <label
          htmlFor="audio-only"
          className="cursor-pointer text-sm font-semibold"
        >
          Audio Only
        </label>
      </div>

      {enabled && (
        <div className="space-y-2">
          <label className="text-sm font-semibold">Audio Format</label>
          <div className="grid grid-cols-3 gap-2">
            {audioFormats.map((fmt) => (
              <Button
                key={fmt}
                variant={selectedFormat === fmt ? "default" : "outline"}
                size="sm"
                onClick={() => onSelectFormat(fmt)}
                className="uppercase"
              >
                {fmt}
              </Button>
            ))}
          </div>
          <p className="text-xs text-muted-foreground">
            Popular formats: MP3 (wide compatibility), M4A (iTunes), OPUS
            (quality)
          </p>
        </div>
      )}
    </section>
  );
}
