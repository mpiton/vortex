import { useTranslation } from "react-i18next";
import { useLanguage } from "@/hooks/useLanguage";
import type { HistoryDayGroup as HistoryDayGroupData } from "./groupByDay";
import { HistoryRow, type HistoryRowActions } from "./HistoryRow";

interface HistoryDayGroupProps {
  group: HistoryDayGroupData;
  actions: HistoryRowActions;
}

function formatDayHeader(unixSeconds: number, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    weekday: "long",
    year: "numeric",
    month: "long",
    day: "numeric",
  }).format(new Date(unixSeconds * 1000));
}

export function HistoryDayGroup({ group, actions }: HistoryDayGroupProps) {
  const { t } = useTranslation();
  const { current: language } = useLanguage();
  const dayHeader = formatDayHeader(group.completedAt, language);

  return (
    <section
      aria-label={t("history.dayGroupAriaLabel", { date: dayHeader })}
      className="flex flex-col"
    >
      <h2 className="sticky top-0 z-10 border-b bg-muted/80 px-3 py-1.5 text-xs font-semibold text-muted-foreground backdrop-blur">
        {dayHeader}
      </h2>
      <table className="w-full text-sm">
        <thead>
          <tr className="border-b text-left text-xs font-medium text-muted-foreground">
            <th scope="col" className="px-3 py-2">
              {t("history.columns.name")}
            </th>
            <th scope="col" className="px-3 py-2">
              {t("history.columns.host")}
            </th>
            <th scope="col" className="px-3 py-2">
              {t("history.columns.size")}
            </th>
            <th scope="col" className="px-3 py-2">
              {t("history.columns.duration")}
            </th>
            <th scope="col" className="px-3 py-2">
              {t("history.columns.completedAt")}
            </th>
            <th scope="col" className="px-3 py-2">
              {t("history.columns.status")}
            </th>
            <th scope="col" className="px-3 py-2">
              {t("history.columns.avgSpeed")}
            </th>
            <th scope="col" className="px-3 py-2">
              {t("history.columns.module")}
            </th>
            <th scope="col" className="px-3 py-2">
              {t("history.columns.account")}
            </th>
            <th scope="col" className="px-3 py-2 text-right">
              <span className="sr-only">{t("history.columns.actions")}</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {group.entries.map((entry) => (
            <HistoryRow key={entry.entryId} entry={entry} actions={actions} />
          ))}
        </tbody>
      </table>
    </section>
  );
}
