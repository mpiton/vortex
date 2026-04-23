import { Button } from '@/components/ui/button';
import type { StatsPeriod } from './derive';

const PERIODS: StatsPeriod[] = ['7d', '30d', 'all'];

export interface PeriodSelectorProps {
  value: StatsPeriod;
  onChange: (next: StatsPeriod) => void;
  ariaLabel: string;
  labels: Record<StatsPeriod, string>;
}

export function PeriodSelector({ value, onChange, ariaLabel, labels }: PeriodSelectorProps) {
  return (
    <div className="flex gap-1.5" role="tablist" aria-label={ariaLabel}>
      {PERIODS.map((period) => (
        <Button
          key={period}
          role="tab"
          aria-selected={value === period}
          variant={value === period ? 'default' : 'ghost'}
          size="sm"
          onClick={() => onChange(period)}
        >
          {labels[period]}
        </Button>
      ))}
    </div>
  );
}
