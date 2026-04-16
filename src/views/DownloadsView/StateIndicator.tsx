import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import type { DownloadState } from '@/types/download';

const stateColors: Record<DownloadState, string> = {
  Queued: 'bg-blue-400',
  Downloading: 'bg-green-500 animate-pulse',
  Paused: 'bg-yellow-400',
  Waiting: 'bg-orange-400',
  Retry: 'bg-orange-500',
  Error: 'bg-red-500',
  Completed: 'bg-emerald-600',
  Checking: 'bg-cyan-500',
  Extracting: 'bg-purple-500',
};

interface StateIndicatorProps {
  state: DownloadState;
  errorMessage?: string | null;
}

export function StateIndicator({ state, errorMessage }: StateIndicatorProps) {
  const showErrorDetails = state === 'Error' && Boolean(errorMessage);

  return (
    <div className="flex items-center gap-2">
      <span className={`h-2.5 w-2.5 rounded-full ${stateColors[state]}`} />
      <span className="text-xs capitalize">{state}</span>
      {showErrorDetails && (
        <Popover>
          <PopoverTrigger asChild>
            <button
              type="button"
              aria-label="Show download error"
              className="flex size-4 items-center justify-center rounded-full border border-red-200 text-[10px] leading-none font-semibold text-red-600 transition-colors hover:border-red-300 hover:bg-red-50"
              onClick={(event) => event.stopPropagation()}
            >
              ?
            </button>
          </PopoverTrigger>
          <PopoverContent
            align="start"
            side="right"
            className="max-w-sm text-xs whitespace-pre-wrap break-words"
            onClick={(event) => event.stopPropagation()}
          >
            {errorMessage}
          </PopoverContent>
        </Popover>
      )}
    </div>
  );
}
