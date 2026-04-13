import { useLayoutStore } from "@/stores/layout-store";
import { useTranslation } from "react-i18next";
import { useDownloadStore, selectTotalSpeed, selectActiveCount } from "@/stores/downloadStore";
import { ClipboardIndicator } from '@/components/ClipboardIndicator';

function Dot() {
  return <span className="text-[10px] text-border">·</span>;
}

export function StatusBar() {
  const { t } = useTranslation();
  const totalSpeed = useDownloadStore(selectTotalSpeed);
  const activeCount = useDownloadStore(selectActiveCount);
  const speedLimit = useLayoutStore((state) => state.speedLimit);
  const freeSpace = useLayoutStore((state) => state.freeSpace);
  const appVersion = useLayoutStore((state) => state.appVersion);

  const limitValue = speedLimit > 0 ? `${speedLimit} MB/s` : t("common.unlimited");

  return (
    <footer className="flex h-[38px] shrink-0 items-center justify-between border-t border-border bg-surface px-6 text-[11px]">
      <div className="flex items-center gap-4">
        <span className="font-semibold text-accent">
          ↓ {totalSpeed.toFixed(1)} MB/s
        </span>
        <Dot />
        <span className="text-text-dim">{t("statusBar.limit", { value: limitValue })}</span>
        <Dot />
        <span className="text-text-dim">{t("statusBar.freeSpace", { value: freeSpace })}</span>
        <Dot />
        <ClipboardIndicator />
      </div>
      <div className="flex items-center gap-4">
        <span className="text-text-ghost">vortex v{appVersion}</span>
        <Dot />
        <div className="flex items-center gap-1.5">
          <div className="h-[7px] w-[7px] rounded-full bg-success" />
          <span className="text-text-dim">{t("statusBar.activeCount", { count: activeCount })}</span>
        </div>
      </div>
    </footer>
  );
}
