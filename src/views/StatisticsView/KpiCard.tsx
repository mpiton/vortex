import type { ComponentType } from 'react';
import { Card, CardContent } from '@/components/ui/card';
import { cn } from '@/lib/utils';

export interface KpiCardProps {
  label: string;
  value: string;
  hint?: string;
  icon?: ComponentType<{ className?: string }>;
  className?: string;
}

export function KpiCard({ label, value, hint, icon: Icon, className }: KpiCardProps) {
  return (
    <Card className={cn('gap-2 py-4', className)}>
      <CardContent className="flex flex-col gap-1.5 px-4">
        <div className="flex items-center justify-between text-xs uppercase tracking-wide text-muted-foreground">
          <span>{label}</span>
          {Icon ? <Icon className="size-3.5 text-muted-foreground/80" /> : null}
        </div>
        <div className="text-xl font-semibold leading-tight">{value}</div>
        {hint ? <div className="text-xs text-muted-foreground">{hint}</div> : null}
      </CardContent>
    </Card>
  );
}
