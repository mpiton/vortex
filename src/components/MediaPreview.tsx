import { useEffect, useState } from "react";

interface MediaPreviewProps {
  title: string;
  thumbnail: string;
  subtitle?: string;
}

export function MediaPreview({ title, thumbnail, subtitle }: MediaPreviewProps) {
  const [imgError, setImgError] = useState(false);

  useEffect(() => {
    setImgError(false);
  }, [thumbnail]);

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
      <div className="space-y-1">
        <p className="text-sm font-semibold">{title}</p>
        {subtitle ? <p className="text-xs text-muted-foreground">{subtitle}</p> : null}
      </div>
    </div>
  );
}
