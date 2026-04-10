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
}

export function StateIndicator({ state }: StateIndicatorProps) {
  return (
    <div className="flex items-center gap-2">
      <span className={`h-2.5 w-2.5 rounded-full ${stateColors[state]}`} />
      <span className="text-xs capitalize">{state}</span>
    </div>
  );
}
