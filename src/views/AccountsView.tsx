import { User } from "lucide-react";

export function AccountsView() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4">
      <User className="h-16 w-16 text-muted-foreground" />
      <div className="text-center">
        <h1 className="text-2xl font-bold">Accounts</h1>
        <p className="mt-1 text-sm text-muted-foreground">Coming soon</p>
      </div>
    </div>
  );
}
