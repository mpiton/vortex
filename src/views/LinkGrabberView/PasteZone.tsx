import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";

interface PasteZoneProps {
  onPasteUrls: (urls: string[]) => void;
  onContainerFiles?: (files: File[]) => void;
  isLoading?: boolean;
  initialValue?: string;
  initialValueToken?: string;
}

export const CONTAINER_EXTENSIONS = [".dlc", ".ccf", ".rsdf", ".metalink", ".meta4"] as const;

export function isContainerFile(file: File): boolean {
  const lower = file.name.toLowerCase();
  return CONTAINER_EXTENSIONS.some((ext) => lower.endsWith(ext));
}

export function extractUrls(text: string): string[] {
  const matches = text.match(/(https?:\/\/[^\s]+|ftp:\/\/[^\s]+|magnet:\?[^\s]+)/gi);
  return (matches ?? []).map((url) => url.replace(/[),.;:>}"'!?]+$/, ""));
}

export function PasteZone({
  onPasteUrls,
  onContainerFiles,
  isLoading,
  initialValue,
  initialValueToken,
}: PasteZoneProps) {
  const { t } = useTranslation();
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const onPasteUrlsRef = useRef(onPasteUrls);
  const handledTokenRef = useRef<string | undefined>(undefined);
  const [isDragging, setIsDragging] = useState(false);

  useEffect(() => {
    onPasteUrlsRef.current = onPasteUrls;
  }, [onPasteUrls]);

  useEffect(() => {
    if (!initialValue || !initialValueToken) return;
    if (initialValueToken === handledTokenRef.current) return;
    if (!textareaRef.current) return;

    handledTokenRef.current = initialValueToken;
    textareaRef.current.value = initialValue;
    const urls = extractUrls(initialValue);
    if (urls.length > 0) {
      onPasteUrlsRef.current(urls);
    }
  }, [initialValue, initialValueToken]);

  function handleAnalyze() {
    const text = textareaRef.current?.value ?? "";
    const urls = extractUrls(text);
    onPasteUrls(urls);
  }

  function handleClear() {
    if (textareaRef.current) {
      textareaRef.current.value = "";
    }
    handledTokenRef.current = undefined;
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
    const containerFiles = files.filter(isContainerFile);

    if (containerFiles.length > 0) {
      if (onContainerFiles) {
        onContainerFiles(containerFiles);
      }
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
