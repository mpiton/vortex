import type { ReactNode } from 'react';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { cn } from '@/lib/utils';

export interface ChartCardProps {
  title: string;
  description?: string;
  children: ReactNode;
  className?: string;
  emptyHint?: string;
  loadingHint?: string;
  isEmpty?: boolean;
  isLoading?: boolean;
}

export function ChartCard({
  title,
  description,
  children,
  className,
  emptyHint,
  loadingHint,
  isEmpty = false,
  isLoading = false,
}: ChartCardProps) {
  const placeholder = isLoading ? (
    <div
      data-testid={`chart-loading-${title.toLowerCase().replace(/\s+/g, '-')}`}
      className="flex h-48 items-center justify-center text-xs text-muted-foreground"
    >
      {loadingHint ?? '…'}
    </div>
  ) : isEmpty ? (
    <div
      data-testid={`chart-empty-${title.toLowerCase().replace(/\s+/g, '-')}`}
      className="flex h-48 items-center justify-center text-xs text-muted-foreground"
    >
      {emptyHint ?? '—'}
    </div>
  ) : null;

  return (
    <Card className={cn('gap-3 py-4', className)}>
      <CardHeader className="px-4">
        <CardTitle className="text-sm font-semibold">{title}</CardTitle>
        {description ? (
          <p className="text-xs text-muted-foreground">{description}</p>
        ) : null}
      </CardHeader>
      <CardContent className="px-4">{placeholder ?? children}</CardContent>
    </Card>
  );
}
