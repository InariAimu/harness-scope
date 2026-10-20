import { command } from "@/lib/client";
import type { CodexAllowanceMode } from "@/types/codexAllowances";
import type {
  AccountRoute,
  AccountSave,
  AccountUsage,
  ProviderAccountView,
  CodexReference,
  CodexPlan,
} from "@/types/providerAccounts";

export const providerAccountsApi = {
  codexEstimate: (args: {
    mode?: CodexAllowanceMode;
    id: string;
    plan: CodexPlan;
    model?: string;
    bucket?: string;
    lookbackMinutes: number;
    pace: string;
  }) => command<CodexReference>("codex_subscription_estimate", args),
  list: () => command<ProviderAccountView[]>("provider_accounts"),
  routes: () => command<AccountRoute[]>("provider_account_routes"),
  save: (account: AccountSave) =>
    command<ProviderAccountView>("provider_account_save", account),
  remove: (id: string) => command<boolean>("provider_account_delete", { id }),
  refresh: (id: string) =>
    command<ProviderAccountView>("provider_account_refresh", { id }),
  activity: (id: string, lease: string, active: boolean) =>
    command<boolean>("provider_account_activity", { id, lease, active }),
  usage: (id: string, range: string) =>
    command<AccountUsage>("provider_account_usage", { id, range }),
};
