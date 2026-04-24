import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";

interface PasteZoneProps {
  onPasteUrls: (urls: string[]) => void;
  isLoading?: boolean;
  initialValue?: string;
}

export function extractUrls(text: string): string[] {
  const matches = text.match(/(https?:\/\/[^\s]+|ftp:\/\/[^\s]+|magnet:\?[^\s]+)/gi);
  return (matches ?? []).map((url) => url.replace(/[),.;:>}"'!?]+$/, ""));
}

export function PasteZone({ onPasteUrls, isLoading, initialValue }: PasteZoneProps) {
  const { t } = useTranslation();
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const onPasteUrlsRef = useRef(onPasteUrls);
  const handledInitialRef = useRef<string | undefined>(undefined);
  const [isDragging, setIsDragging] = useState(false);

  useEffect(() => {
    onPasteUrlsRef.current = onPasteUrls;
  }, [onPasteUrls]);

  useEffect(() => {
    if (!initialValue || initialValue === handledInitialRef.current) return;
    if (!textareaRef.current) return;

    handledInitialRef.current = initialValue;
    textareaRef.current.value = initialValue;
    const urls = extractUrls(initialValue);
    if (urls.length > 0) {
      onPasteUrlsRef.current(urls);
    }
  }, [initialValue]);

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
        isDragging ? "border-accent bg-accent/10" : "border-muted-foreground/30"
      }`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      <textarea
        data-shortcut-target="link-grabber-paste"
        ref={textareaRef}
        className="h-32 w-full resize-none rounded border bg-background p-3 text-sm focus:outline-none focus:ring-2 focus:ring-accent"
        placeholder={t("linkGrabber.pastePlaceholder")}
      />
      <div className="mt-3 flex gap-2">
        <Button variant="outline" onClick={handleClear}>
          {t("common.clear")}
        </Button>
        <Button onClick={handleAnalyze} disabled={isLoading}>
          {isLoading ? t("linkGrabber.resolving") : t("linkGrabber.analyze")}
        </Button>
      </div>
    </div>
  );
}
