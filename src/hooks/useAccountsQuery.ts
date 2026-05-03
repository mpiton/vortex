import { useQuery } from "@tanstack/react-query";
import { tauriInvoke } from "@/api/client";
import { accountQueries } from "@/api/queries";
import type { AccountListFilter, AccountView } from "@/types/account";

export function useAccountsQuery(filter?: AccountListFilter) {
  return useQuery<AccountView[], Error>({
    queryKey: filter ? accountQueries.list(filter) : accountQueries.lists(),
    queryFn: () =>
      tauriInvoke<AccountView[]>("account_list", {
        serviceName: filter?.serviceName,
        accountType: filter?.accountType,
        enabled: filter?.enabled,
      }),
    staleTime: 30_000,
  });
}
