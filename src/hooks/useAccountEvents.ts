import { queryClient } from "@/api/client";
import { accountQueries } from "@/api/queries";
import { useTauriEvent } from "@/hooks/useTauriEvent";
import type {
  AccountAddedPayload,
  AccountExhaustedPayload,
  AccountIdPayload,
  AccountValidatedPayload,
  AccountValidationFailedPayload,
} from "@/types/events";

export function useAccountEvents(): void {
  const invalidateAccounts = () => {
    queryClient.invalidateQueries({ queryKey: accountQueries.all() });
  };

  useTauriEvent<AccountAddedPayload>("account-added", invalidateAccounts);
  useTauriEvent<AccountIdPayload>("account-updated", invalidateAccounts);
  useTauriEvent<AccountIdPayload>("account-deleted", invalidateAccounts);
  useTauriEvent<AccountValidatedPayload>("account-validated", invalidateAccounts);
  useTauriEvent<AccountValidationFailedPayload>("account-validation-failed", invalidateAccounts);
  useTauriEvent<AccountExhaustedPayload>("account-exhausted", invalidateAccounts);
}
