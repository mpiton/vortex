import { useTranslation } from 'react-i18next';
import { useLanguage } from '@/hooks/useLanguage';
import type { HistoryDayGroup as HistoryDayGroupData } from './groupByDay';
import { HistoryRow, type HistoryRowActions } from './HistoryRow';

interface HistoryDayGroupProps {
  group: HistoryDayGroupData;
  actions: HistoryRowActions;
}

function formatDayHeader(unixSeconds: number, locale: string): string {
  return new Intl.DateTimeFormat(locale, {
    weekday: 'long',
    year: 'numeric',
    month: 'long',
    day: 'numeric',
  }).format(new Date(unixSeconds * 1000));
}

export function HistoryDayGroup({ group, actions }: HistoryDayGroupProps) {
  const { t } = useTranslation();
  const { current: language } = useLanguage();

  return (
    <section
      aria-label={t('history.dayGroupAriaLabel', {
        date: formatDayHeader(group.completedAt, language),
      })}
      className="flex flex-col"
    >
      <h2 className="sticky top-0 z-10 border-b bg-muted/80 px-3 py-1.5 text-xs font-semibold text-muted-foreground backdrop-blur">
        {formatDayHeader(group.completedAt, language)}
      </h2>
      <table className="w-full text-sm">
        <tbody>
          {group.entries.map((entry) => (
            <HistoryRow key={entry.entryId} entry={entry} actions={actions} />
          ))}
        </tbody>
      </table>
    </section>
  );
}
