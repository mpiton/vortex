import { Loader2, Check, X, AlertCircle } from "lucide-react";
import { Checkbox } from "@/components/ui/checkbox";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { formatBytes } from "@/lib/format";
import type { ResolvedLink } from "./types";

interface LinkRowProps {
  link: ResolvedLink;
  selected: boolean;
  onSelect: () => void;
  onMediaClick?: (link: ResolvedLink) => void;
}

const statusIconMap = {
  checking: <Loader2 className="h-4 w-4 animate-spin text-blue-500" />,
  online: <Check className="h-4 w-4 text-green-600" />,
  offline: <X className="h-4 w-4 text-red-500" />,
  error: <AlertCircle className="h-4 w-4 text-yellow-500" />,
};

export function LinkRow({ link, selected, onSelect, onMediaClick }: LinkRowProps) {
  return (
    <div
      className={`flex items-center gap-3 rounded p-2 transition-colors hover:bg-muted ${
        selected ? "bg-accent/20" : ""
      }`}
    >
      <Checkbox
        checked={selected}
        onCheckedChange={onSelect}
        aria-label="Select link"
      />
      {statusIconMap[link.status]}
      <Tooltip>
        <TooltipTrigger asChild>
          <span
            tabIndex={0}
            className="min-w-0 flex-1 truncate text-sm font-semibold"
          >
            {link.filename || link.originalUrl}
          </span>
        </TooltipTrigger>
        <TooltipContent>{link.originalUrl}</TooltipContent>
      </Tooltip>
      <span className="shrink-0 text-xs text-muted-foreground">
        {link.moduleName}
      </span>
      {link.sizeBytes !== null && (
        <span className="shrink-0 text-xs text-muted-foreground">
          {formatBytes(link.sizeBytes)}
        </span>
      )}
      {link.isMedia && (
        <Button
          variant="outline"
          size="sm"
          className="h-6 px-2 text-xs"
          onClick={(e) => {
            e.stopPropagation();
            onMediaClick?.(link);
          }}
        >
          {link.mediaType ?? "Media"}
        </Button>
      )}
    </div>
  );
}
