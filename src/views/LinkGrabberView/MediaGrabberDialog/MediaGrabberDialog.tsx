import { useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { MediaPreview } from "@/components/MediaPreview";
import { QualitySelector } from "./QualitySelector";
import { AudioOnlySection } from "./AudioOnlySection";
import { SubtitleSelector } from "./SubtitleSelector";
import { PlaylistSection } from "./PlaylistSection";
import { SizeEstimate } from "./SizeEstimate";
import { useMediaMetadata } from "./useMediaMetadata";
import type { ResolvedLink } from "../types";
import type { MediaGrabberOptions } from "@/types/media";

interface MediaGrabberDialogProps {
  link: ResolvedLink;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: (options: MediaGrabberOptions) => void;
}

export function MediaGrabberDialog({
  link,
  open,
  onOpenChange,
  onConfirm,
}: MediaGrabberDialogProps) {
  const [qualitySelection, setQualitySelection] = useState("1080p");
  const [formatSelection, setFormatSelection] = useState("mp4");
  const [audioOnly, setAudioOnly] = useState(false);
  const [audioFormat, setAudioFormat] = useState("m4a");
  const [selectedSubtitles, setSelectedSubtitles] = useState<string[]>([]);
  const [selectedPlaylistItems, setSelectedPlaylistItems] = useState<string[]>(
    [],
  );

  const {
    data: metadata,
    isLoading,
    isError,
    refetch,
  } = useMediaMetadata(link.originalUrl, open);

  const handleConfirm = () => {
    onConfirm({
      quality: audioOnly ? "audio_only" : qualitySelection,
      format: audioOnly ? audioFormat : formatSelection,
      subtitles: selectedSubtitles,
      audioOnly,
      playlistItems: selectedPlaylistItems,
    });
    onOpenChange(false);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[80vh] max-w-2xl overflow-y-auto">
        <DialogHeader>
          <DialogTitle>Media Grabber Options</DialogTitle>
        </DialogHeader>

        {isLoading ? (
          <Skeleton className="h-64" />
        ) : metadata ? (
          <div className="space-y-6">
            <MediaPreview
              title={metadata.title}
              thumbnail={metadata.thumbnailUrl}
            />

            {metadata.isPlaylist && metadata.playlistItems && (
              <PlaylistSection
                items={metadata.playlistItems}
                selectedItems={selectedPlaylistItems}
                onSelectItems={setSelectedPlaylistItems}
              />
            )}

            <div className="space-y-4 border-t pt-6">
              {!audioOnly && metadata.availableQualities.length > 0 && (
                <QualitySelector
                  qualities={metadata.availableQualities}
                  formats={metadata.availableFormats}
                  selected={qualitySelection}
                  selectedFormat={formatSelection}
                  onSelectQuality={setQualitySelection}
                  onSelectFormat={setFormatSelection}
                />
              )}

              <AudioOnlySection
                enabled={audioOnly}
                onEnabledChange={setAudioOnly}
                audioFormats={metadata.availableAudioFormats}
                selectedFormat={audioFormat}
                onSelectFormat={setAudioFormat}
              />

              {metadata.availableSubtitles.length > 0 && (
                <SubtitleSelector
                  languages={metadata.availableSubtitles}
                  selected={selectedSubtitles}
                  onSelect={setSelectedSubtitles}
                />
              )}
            </div>

            <SizeEstimate
              quality={audioOnly ? "audio_only" : qualitySelection}
              format={audioOnly ? audioFormat : formatSelection}
              duration={metadata.durationSeconds}
            />
          </div>
        ) : (
          <div className="space-y-3 text-center">
            <p className="text-sm text-muted-foreground">
              {isError
                ? "Failed to load media metadata"
                : "No metadata available for this link"}
            </p>
            {isError && (
              <Button variant="outline" size="sm" onClick={() => refetch()}>
                Retry
              </Button>
            )}
          </div>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={handleConfirm} disabled={isLoading || !metadata}>
            Download
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
