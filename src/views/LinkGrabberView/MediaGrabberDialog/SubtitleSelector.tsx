import { Checkbox } from "@/components/ui/checkbox";
import type { SubtitleLanguage } from "@/types/media";

interface SubtitleSelectorProps {
  languages: SubtitleLanguage[];
  selected: string[];
  onSelect: (codes: string[]) => void;
}

export function SubtitleSelector({
  languages,
  selected,
  onSelect,
}: SubtitleSelectorProps) {
  return (
    <section className="space-y-3">
      <h3 className="text-sm font-semibold">Subtitles</h3>
      <p className="text-xs text-muted-foreground">
        Select languages to download (if available)
      </p>
      <div className="grid max-h-48 grid-cols-2 gap-2 overflow-y-auto">
        {languages.map((lang) => (
          <div key={lang.code} className="flex items-center gap-2">
            <Checkbox
              id={`subtitle-${lang.code}`}
              checked={selected.includes(lang.code)}
              onCheckedChange={(checked) => {
                const next = new Set(selected);
                if (checked === true) next.add(lang.code);
                else next.delete(lang.code);
                onSelect([...next]);
              }}
            />
            <label
              htmlFor={`subtitle-${lang.code}`}
              className="cursor-pointer text-sm"
            >
              {lang.name} ({lang.code})
            </label>
          </div>
        ))}
      </div>
    </section>
  );
}
