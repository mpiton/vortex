import { useTranslation } from 'react-i18next';
import { Badge } from '@/components/ui/badge';
import { cn } from '@/lib/utils';
import type { HistoryStatus } from './filterEntries';

const STATUS_STYLES: Record<HistoryStatus, string> = {
  completed: 'border-transparent bg-emerald-600 text-white hover:bg-emerald-600/80',
  failed: 'border-transparent bg-red-600 text-white hover:bg-red-600/80',
  cancelled: 'border-transparent bg-orange-500 text-white hover:bg-orange-500/80',
};

interface StatusBadgeProps {
  status: HistoryStatus;
  className?: string;
}

export function StatusBadge({ status, className }: StatusBadgeProps) {
  const { t } = useTranslation();

  return (
    <Badge className={cn(STATUS_STYLES[status], className)}>
      {t(`history.status.${status}`)}
    </Badge>
  );
}
