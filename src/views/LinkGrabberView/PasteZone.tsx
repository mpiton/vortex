import { useRef, useState } from "react";
import { Button } from "@/components/ui/button";

interface PasteZoneProps {
  onPasteUrls: (urls: string[]) => void;
  isLoading?: boolean;
}

export function extractUrls(text: string): string[] {
  const matches = text.match(
    /(https?:\/\/[^\s]+|ftp:\/\/[^\s]+|magnet:\?[^\s]+)/gi,
  );
  return (matches ?? []).map((url) => url.replace(/[),.;:>\]}"'!?]+$/, ""));
}

export function PasteZone({ onPasteUrls, isLoading }: PasteZoneProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const [isDragging, setIsDragging] = useState(false);

  function handleAnalyze() {
    const text = textareaRef.current?.value ?? "";
    const urls = extractUrls(text);
    onPasteUrls(urls);
  }

  function handleClear() {
    if (textareaRef.current) {
      textareaRef.current.value = "";
    }
  }

  function handleDragOver(e: React.DragEvent<HTMLDivElement>) {
    e.preventDefault();
    setIsDragging(true);
  }

  function handleDragLeave() {
    setIsDragging(false);
  }

  function handleDrop(e: React.DragEvent<HTMLDivElement>) {
    e.preventDefault();
    setIsDragging(false);

    const files = Array.from(e.dataTransfer?.files ?? []);
    const containerExtensions = [".dlc", ".ccf", ".rsdf", ".metalink"];
    const containerFiles = files.filter((f) =>
      containerExtensions.some((ext) => f.name.toLowerCase().endsWith(ext)),
    );

    if (containerFiles.length > 0) {
      const containerUrls = containerFiles.map((f) => `container:${f.name}`);
      onPasteUrls(containerUrls);
      return;
    }

    const text = e.dataTransfer?.getData("text") ?? "";
    const urls = extractUrls(text);
    if (urls.length > 0) {
      onPasteUrls(urls);
      if (textareaRef.current) {
        textareaRef.current.value = text;
      }
    }
  }

  return (
    <div
      data-testid="paste-drop-zone"
      className={`rounded-lg border-2 border-dashed p-6 transition-colors ${
        isDragging
          ? "border-accent bg-accent/10"
          : "border-muted-foreground/30"
      }`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      <textarea
        ref={textareaRef}
        className="h-32 w-full resize-none rounded border bg-background p-3 text-sm focus:outline-none focus:ring-2 focus:ring-accent"
        placeholder="Paste URLs here (one per line)…"
      />
      <div className="mt-3 flex gap-2">
        <Button variant="outline" onClick={handleClear}>
          Clear
        </Button>
        <Button onClick={handleAnalyze} disabled={isLoading}>
          {isLoading ? "Resolving…" : "Analyze Links"}
        </Button>
      </div>
    </div>
  );
}
