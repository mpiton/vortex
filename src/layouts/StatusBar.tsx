import { useLayoutStore } from "@/stores/layout-store";

function Dot() {
  return <span className="text-[10px] text-border">·</span>;
}

export function StatusBar() {
  const totalSpeed = useLayoutStore((state) => state.totalSpeed);
  const speedLimit = useLayoutStore((state) => state.speedLimit);
  const freeSpace = useLayoutStore((state) => state.freeSpace);
  const totalConnections = useLayoutStore((state) => state.totalConnections);
  const appVersion = useLayoutStore((state) => state.appVersion);

  const limitLabel = speedLimit > 0 ? `${speedLimit} MB/s` : "unlimited";

  return (
    <footer className="flex h-[38px] shrink-0 items-center justify-between border-t border-border bg-surface px-6 text-[11px]">
      <div className="flex items-center gap-4">
        <span className="font-semibold text-accent">
          ↓ {totalSpeed.toFixed(1)} MB/s
        </span>
        <Dot />
        <span className="text-text-dim">Limit: {limitLabel}</span>
        <Dot />
        <span className="text-text-dim">{freeSpace} free</span>
      </div>
      <div className="flex items-center gap-4">
        <span className="text-text-ghost">vortex v{appVersion}</span>
        <Dot />
        <div className="flex items-center gap-1.5">
          <div className="h-[7px] w-[7px] rounded-full bg-success" />
          <span className="text-text-dim">{totalConnections} connections</span>
        </div>
      </div>
    </footer>
  );
}
