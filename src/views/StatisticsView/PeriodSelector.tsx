import { useEffect, useRef } from 'react';
import type { KeyboardEvent } from 'react';
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
  const containerRef = useRef<HTMLDivElement | null>(null);
  const shouldFocusSelected = useRef(false);

  useEffect(() => {
    if (!shouldFocusSelected.current) return;
    shouldFocusSelected.current = false;
    const selected = containerRef.current?.querySelector<HTMLButtonElement>(
      '[role="tab"][aria-selected="true"]',
    );
    selected?.focus();
  }, [value]);

  function handleKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const current = PERIODS.indexOf(value);
    if (current === -1) return;
    let next = current;
    switch (event.key) {
      case 'ArrowRight':
      case 'ArrowDown':
        next = (current + 1) % PERIODS.length;
        break;
      case 'ArrowLeft':
      case 'ArrowUp':
        next = (current - 1 + PERIODS.length) % PERIODS.length;
        break;
      case 'Home':
        next = 0;
        break;
      case 'End':
        next = PERIODS.length - 1;
        break;
      default:
        return;
    }
    event.preventDefault();
    if (next === current) return;
    shouldFocusSelected.current = true;
    onChange(PERIODS[next]);
  }

  return (
    <div
      ref={containerRef}
      className="flex gap-1.5"
      role="tablist"
      aria-label={ariaLabel}
      onKeyDown={handleKeyDown}
    >
      {PERIODS.map((period) => (
        <Button
          key={period}
          role="tab"
          aria-selected={value === period}
          tabIndex={value === period ? 0 : -1}
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
