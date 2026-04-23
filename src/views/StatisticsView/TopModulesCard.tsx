import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import type { ModuleStats } from '@/types/download';
import { formatBytes, formatCount } from './format';

export interface TopModulesCardProps {
  data: ModuleStats[] | undefined;
  title: string;
  emptyHint: string;
  countLabel: string;
  loadingHint?: string;
}

export function TopModulesCard({
  data,
  title,
  emptyHint,
  countLabel,
  loadingHint,
}: TopModulesCardProps) {
  const isLoading = data === undefined;
  const entries = data ?? [];
  return (
    <Card className="gap-3 py-4">
      <CardHeader className="px-4">
        <CardTitle className="text-sm font-semibold">{title}</CardTitle>
      </CardHeader>
      <CardContent className="px-4">
        {isLoading ? (
          <div
            data-testid="top-modules-loading"
            className="flex h-32 items-center justify-center text-xs text-muted-foreground"
          >
            {loadingHint ?? '…'}
          </div>
        ) : entries.length === 0 ? (
          <div
            data-testid="top-modules-empty"
            className="flex h-32 items-center justify-center text-xs text-muted-foreground"
          >
            {emptyHint}
          </div>
        ) : (
          <ul className="flex flex-col divide-y divide-border">
            {entries.map((module, index) => (
              <li
                key={module.moduleName}
                className="flex items-center justify-between gap-3 py-2 text-sm"
              >
                <div className="flex items-center gap-3">
                  <span className="w-5 text-right text-xs text-muted-foreground">
                    {index + 1}
                  </span>
                  <span className="font-medium">{module.moduleName}</span>
                </div>
                <div className="flex flex-col items-end gap-0.5 text-xs text-muted-foreground">
                  <span>
                    {formatCount(module.downloadCount)} {countLabel}
                  </span>
                  <span>{formatBytes(module.totalBytes)}</span>
                </div>
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}
