import { BarChart3 } from "lucide-react";

export function StatisticsView() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4">
      <BarChart3 className="h-16 w-16 text-muted-foreground" />
      <div className="text-center">
        <h1 className="text-2xl font-bold">Statistics</h1>
        <p className="mt-1 text-sm text-muted-foreground">Coming soon</p>
      </div>
    </div>
  );
}
