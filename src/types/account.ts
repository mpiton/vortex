export type AccountType = "free" | "premium" | "debrid";
export type PersistedAccountStatus =
  | "unverified"
  | "valid"
  | "invalid_credentials"
  | "missing_credential"
  | "expired"
  | "quota_exhausted"
  | "cooldown"
  | "error";

export interface AccountView {
  id: string;
  serviceName: string;
  username: string;
  accountType: AccountType;
  enabled: boolean;
  trafficLeft: number | null;
  trafficTotal: number | null;
  validUntil: number | null;
  lastValidated: number | null;
  createdAt: number;
  status: PersistedAccountStatus;
  exhaustedUntil: number | null;
  credentialRef: string;
}

export interface AccountTraffic {
  id: string;
  trafficLeft: number | null;
  trafficTotal: number | null;
  validUntil: number | null;
  lastValidated: number | null;
}

export interface AccountPatch {
  username?: string;
  password?: string;
  accountType?: AccountType;
  enabled?: boolean;
}

export interface AddAccountInput {
  serviceName: string;
  username: string;
  password: string;
  accountType: AccountType;
}

export interface ValidationOutcome {
  valid: boolean;
  latencyMs: number | null;
  trafficLeft: number | null;
  trafficTotal: number | null;
  validUntil: number | null;
  errorMessage: string | null;
}

export interface ExportAccountsResult {
  path: string;
  count: number;
}

export interface ImportAccountsResult {
  path: string;
  imported: number;
  skippedDuplicates: number;
}

export interface AccountListFilter {
  serviceName?: string;
  accountType?: AccountType;
  enabled?: boolean;
}
