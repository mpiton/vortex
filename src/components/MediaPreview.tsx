import { useState } from "react";

interface MediaPreviewProps {
  title: string;
  thumbnail: string;
}

export function MediaPreview({ title, thumbnail }: MediaPreviewProps) {
  const [imgError, setImgError] = useState(false);

  return (
    <div className="space-y-2">
      {imgError ? (
        <div className="flex w-full items-center justify-center rounded bg-muted aspect-video">
          <span className="text-sm text-muted-foreground">No preview available</span>
        </div>
      ) : (
        <img
          src={thumbnail}
          alt={title}
          className="w-full rounded object-cover aspect-video"
          onError={() => setImgError(true)}
        />
      )}
      <p className="text-sm font-semibold">{title}</p>
    </div>
  );
}
