import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Activity,
  Award,
  CheckCircle2,
  Clock,
  Files,
  Gauge,
  HardDrive,
  ShieldCheck,
} from "lucide-react";
import { ChartCard } from "./ChartCard";
import { DailyVolumeChart } from "./DailyVolumeChart";
import {
  deriveSpeedSeries,
  deriveTypeBreakdown,
  filterEntriesByPeriod,
  type StatsPeriod,
} from "./derive";
import { KpiCard } from "./KpiCard";
import { PeriodSelector } from "./PeriodSelector";
import { SpeedCurveChart } from "./SpeedCurveChart";
import { TopHostsChart } from "./TopHostsChart";
import { TopModulesCard } from "./TopModulesCard";
import { TypeBreakdownChart } from "./TypeBreakdownChart";
import { useStatsQuery } from "@/hooks/useStatsQuery";
import {
  formatBytes,
  formatCount,
  formatDurationFromSeconds,
  formatPercent,
  formatSpeed,
} from "./format";

const DEFAULT_PERIOD: StatsPeriod = "7d";

function nowSeconds(): number {
  return Math.floor(Date.now() / 1000);
}

function sumDurations(entries: { durationSeconds: number }[]): number {
  return entries.reduce((acc, entry) => acc + entry.durationSeconds, 0);
}

export function StatisticsView() {
  const { t } = useTranslation();
  const [period, setPeriod] = useState<StatsPeriod>(DEFAULT_PERIOD);
  const { stats, topModules, history, isLoading, error, historyStatus, topModulesStatus } =
    useStatsQuery(period);

  const periodLabels = useMemo<Record<StatsPeriod, string>>(
    () => ({
      "7d": t("statistics.period.7d"),
      "30d": t("statistics.period.30d"),
      all: t("statistics.period.all"),
    }),
    [t],
  );

  const filteredHistory = useMemo(
    () => (history ? filterEntriesByPeriod(history, period, nowSeconds()) : undefined),
    [history, period],
  );

  const typeBreakdown = useMemo(
    () => (filteredHistory ? deriveTypeBreakdown(filteredHistory) : undefined),
    [filteredHistory],
  );
  const speedSeries = useMemo(
    () => (filteredHistory ? deriveSpeedSeries(filteredHistory) : undefined),
    [filteredHistory],
  );
  const timeSavedSeconds = useMemo(
    () => (filteredHistory ? sumDurations(filteredHistory) : undefined),
    [filteredHistory],
  );

  if (!stats && isLoading) {
    return (
      <div className="flex h-full items-center justify-center p-6 text-sm text-muted-foreground">
        {t("statistics.loading")}
      </div>
    );
  }

  if (!stats && error) {
    return (
      <div
        role="alert"
        className="flex h-full items-center justify-center p-6 text-sm text-destructive"
      >
        {error.message}
      </div>
    );
  }

  return (
    <div
      className="flex h-full min-h-0 flex-col gap-4 overflow-auto p-4"
      data-testid="statistics-view"
    >
      <header className="flex items-center justify-between gap-3">
        <div>
          <h1 className="text-base font-semibold">{t("statistics.title")}</h1>
          <p className="text-xs text-muted-foreground">{t("statistics.description")}</p>
        </div>
        <PeriodSelector
          value={period}
          onChange={setPeriod}
          ariaLabel={t("statistics.period.ariaLabel")}
          labels={periodLabels}
        />
      </header>

      {error ? (
        <div
          role="alert"
          className="rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive"
        >
          {error.message}
        </div>
      ) : null}

      <section
        aria-label={t("statistics.kpi.ariaLabel")}
        className="grid grid-cols-2 gap-3 md:grid-cols-4 xl:grid-cols-7"
      >
        <KpiCard
          label={t("statistics.kpi.totalVolume")}
          value={formatBytes(stats?.totalDownloadedBytes ?? 0)}
          icon={HardDrive}
        />
        <KpiCard
          label={t("statistics.kpi.totalFiles")}
          value={formatCount(stats?.totalFiles ?? 0)}
          icon={Files}
        />
        <KpiCard
          label={t("statistics.kpi.avgSpeed")}
          value={formatSpeed(stats?.avgSpeed ?? 0)}
          icon={Activity}
        />
        <KpiCard
          label={t("statistics.kpi.peakSpeed")}
          value={formatSpeed(stats?.peakSpeed ?? 0)}
          icon={Gauge}
        />
        <KpiCard
          label={t("statistics.kpi.successRate")}
          value={formatPercent(stats?.successRate ?? 0)}
          icon={CheckCircle2}
        />
        <KpiCard
          label={t("statistics.kpi.timeSaved")}
          value={
            historyStatus.isLoading
              ? "…"
              : timeSavedSeconds === undefined
                ? "—"
                : formatDurationFromSeconds(timeSavedSeconds)
          }
          hint={t("statistics.kpi.timeSavedHint")}
          icon={Clock}
        />
        <KpiCard
          label={t("statistics.kpi.captchasSolved")}
          value="0"
          hint={t("statistics.kpi.captchasSolvedHint")}
          icon={ShieldCheck}
        />
      </section>

      <section className="grid grid-cols-1 gap-3 lg:grid-cols-2">
        <ChartCard
          title={t("statistics.charts.dailyVolume.title")}
          description={t("statistics.charts.dailyVolume.description")}
          isEmpty={(stats?.dailyVolumes ?? []).length === 0}
          emptyHint={t("statistics.charts.empty")}
        >
          <DailyVolumeChart
            data={stats?.dailyVolumes ?? []}
            ariaLabel={t("statistics.charts.dailyVolume.ariaLabel")}
            xAxisLabel={t("statistics.charts.dailyVolume.xAxis")}
            yAxisLabel={t("statistics.charts.dailyVolume.yAxis")}
          />
        </ChartCard>

        <ChartCard
          title={t("statistics.charts.topHosts.title")}
          description={t("statistics.charts.topHosts.description")}
          isEmpty={(stats?.topHosts ?? []).length === 0}
          emptyHint={t("statistics.charts.empty")}
        >
          <TopHostsChart
            data={stats?.topHosts ?? []}
            ariaLabel={t("statistics.charts.topHosts.ariaLabel")}
            bytesLabel={t("statistics.charts.topHosts.bytesLabel")}
          />
        </ChartCard>

        <ChartCard
          title={t("statistics.charts.typeBreakdown.title")}
          description={t("statistics.charts.typeBreakdown.description")}
          isLoading={historyStatus.isLoading}
          isEmpty={!typeBreakdown || typeBreakdown.length === 0}
          emptyHint={t("statistics.charts.empty")}
          loadingHint={t("statistics.loading")}
        >
          {typeBreakdown && typeBreakdown.length > 0 ? (
            <TypeBreakdownChart
              data={typeBreakdown}
              ariaLabel={t("statistics.charts.typeBreakdown.ariaLabel")}
              bytesLabel={t("statistics.charts.typeBreakdown.bytesLabel")}
            />
          ) : null}
        </ChartCard>

        <ChartCard
          title={t("statistics.charts.speedCurve.title")}
          description={t("statistics.charts.speedCurve.description")}
          isLoading={historyStatus.isLoading}
          isEmpty={!speedSeries || speedSeries.length === 0}
          emptyHint={t("statistics.charts.empty")}
          loadingHint={t("statistics.loading")}
        >
          {speedSeries && speedSeries.length > 0 ? (
            <SpeedCurveChart
              data={speedSeries}
              ariaLabel={t("statistics.charts.speedCurve.ariaLabel")}
              xAxisLabel={t("statistics.charts.speedCurve.xAxis")}
              yAxisLabel={t("statistics.charts.speedCurve.yAxis")}
            />
          ) : null}
        </ChartCard>
      </section>

      <section>
        <TopModulesCard
          data={topModules}
          isLoading={topModulesStatus.isLoading}
          title={t("statistics.topModules.title")}
          emptyHint={t("statistics.topModules.empty")}
          countLabel={t("statistics.topModules.countLabel")}
          loadingHint={t("statistics.loading")}
        />
        <div className="mt-2 flex items-center gap-2 text-xs text-muted-foreground">
          <Award className="size-3.5" />
          {t("statistics.topModules.hint")}
        </div>
      </section>
    </div>
  );
}
