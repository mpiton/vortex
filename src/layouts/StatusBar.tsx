import { useLayoutStore } from "@/stores/layout-store";

export function StatusBar() {
  const totalSpeed = useLayoutStore((state) => state.totalSpeed);
  const speedLimit = useLayoutStore((state) => state.speedLimit);
  const freeSpace = useLayoutStore((state) => state.freeSpace);
  const activeCount = useLayoutStore((state) => state.activeCount);
  const totalConnections = useLayoutStore((state) => state.totalConnections);
  const appVersion = useLayoutStore((state) => state.appVersion);

  return (
    <footer className="flex items-center justify-between border-t bg-secondary px-4 py-1.5 text-xs text-muted-foreground">
      <span>{activeCount} active</span>
      <span>
        ↓ {totalSpeed.toFixed(1)} MB/s
        {speedLimit > 0 ? ` / ${speedLimit} MB/s` : ""}
      </span>
      <span>{freeSpace} free</span>
      <span>{totalConnections} conn.</span>
      <span>v{appVersion}</span>
    </footer>
  );
}
