import { useTranslation } from "react-i18next";
import { useDownloadStore, selectTotalSpeed, selectActiveCount } from "@/stores/downloadStore";
import { useTauriQuery } from "@/api/hooks";
import { ClipboardIndicator } from '@/components/ClipboardIndicator';
import { useLayoutStore } from "@/stores/layout-store";
import { useSettingsStore } from "@/stores/settingsStore";

interface StatusBarData {
  freeSpaceBytes: number | null;
}

function formatFreeSpace(bytes: number | null | undefined, locale: string) {
  if (bytes === null || bytes === undefined) return "-- GB";

  const numberFormat = new Intl.NumberFormat(locale, {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
  });

  if (bytes >= 1_000_000_000) {
    return `${numberFormat.format(bytes / 1_000_000_000)} GB`;
  }

  return `${numberFormat.format(bytes / 1_000_000)} MB`;
}

function Dot() {
  return <span className="text-[10px] text-border">·</span>;
}

export function StatusBar() {
  const { t, i18n } = useTranslation();
  const totalSpeed = useDownloadStore(selectTotalSpeed);
  const activeCount = useDownloadStore(selectActiveCount);
  const speedLimitBytesPerSec = useSettingsStore((state) => state.config?.speedLimitBytesPerSec);
  const appVersion = useLayoutStore((state) => state.appVersion);
  const { data: statusBarData } = useTauriQuery<StatusBarData>(
    "status_bar_get",
    undefined,
    { queryKey: ["status_bar_get"], staleTime: 30_000 },
  );

  const limitValue = speedLimitBytesPerSec && speedLimitBytesPerSec > 0
    ? `${(speedLimitBytesPerSec / 1_048_576).toFixed(1)} MB/s`
    : t("common.unlimited");
  const freeSpace = formatFreeSpace(
    statusBarData?.freeSpaceBytes,
    i18n.resolvedLanguage ?? i18n.language,
  );

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
