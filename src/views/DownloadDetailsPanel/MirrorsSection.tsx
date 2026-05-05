import type { DownloadDetailView } from "@/types/download";

interface MirrorsSectionProps {
  download: DownloadDetailView;
}

function getHostname(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return url;
  }
}

export function MirrorsSection({ download }: MirrorsSectionProps) {
  if (!download.mirrors || download.mirrors.length === 0) {
    return null;
  }

  const activeIndex = download.currentMirrorIndex;
  const active = download.mirrors[activeIndex];

  return (
    <section className="space-y-3" aria-label="Metalink mirrors">
      <h3 className="text-sm font-semibold">Mirrors</h3>
      {active ? (
        <div className="space-y-1 text-xs">
          <p className="text-muted-foreground">Active</p>
          <p className="font-mono">{getHostname(active.url)}</p>
          <p className="text-muted-foreground">
            Priority {active.priority}
            {active.country ? ` · ${active.country}` : ""}
          </p>
        </div>
      ) : null}
      <div className="space-y-1 text-xs">
        <p className="text-muted-foreground">Alternatives ({download.mirrors.length})</p>
        <ul className="space-y-1">
          {download.mirrors.map((mirror, index) => {
            const isActive = index === activeIndex;
            return (
              <li
                key={`${mirror.url}-${index}`}
                className={`flex items-center justify-between gap-2 rounded px-2 py-1 ${
                  isActive ? "bg-muted font-medium" : ""
                }`}
                aria-current={isActive ? "true" : undefined}
              >
                <span className="font-mono truncate">{getHostname(mirror.url)}</span>
                <span className="shrink-0 text-muted-foreground">
                  P{mirror.priority}
                  {mirror.country ? ` ${mirror.country}` : ""}
                </span>
              </li>
            );
          })}
        </ul>
      </div>
    </section>
  );
}
