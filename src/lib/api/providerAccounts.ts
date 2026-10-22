import { command } from "@/lib/client";
import type { CodexAllowanceMode } from "@/types/codexAllowances";
import type {
  AccountRoute,
  AccountSave,
  AccountUsage,
  AccountUsageRange,
  ProviderAccountView,
  ProviderAccountHistory,
  ProviderAccountHistoryQuery,
  ProviderAccountEstimate,
  ProviderAccountEstimateQuery,
  CodexReference,
  CodexPlan,
  CustomQuerySettings,
  CustomUsage,
  ProviderSessionSource,
} from "@/types/providerAccounts";

export const providerAccountsApi = {
  sharedSubscriptions: () => command<{ id: string; provider: string; name: string }[]>("shared_subscriptions"),
  testQuery: (args: { id?: string; settings: CustomQuerySettings; apiKey?: string; clearApiKey?: boolean }) =>
    command<CustomUsage>("provider_account_test_query", args),
  sessionSources: (harness: string, sessionId: string) =>
    command<ProviderSessionSource[]>("provider_session_sources", { harness, sessionId }),
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
  sidebarOrder: () => command<string[]>("provider_sidebar_order"),
  saveSidebarOrder: (ids: string[]) =>
    command<string[]>("save_provider_sidebar_order", { ids }),
  history: (args: ProviderAccountHistoryQuery) =>
    command<ProviderAccountHistory>("provider_account_history", args),
  estimate: (args: ProviderAccountEstimateQuery) =>
    command<ProviderAccountEstimate>("provider_account_estimate", args),
  routes: () => command<AccountRoute[]>("provider_account_routes"),
  save: (account: AccountSave) =>
    command<ProviderAccountView>("provider_account_save", account),
  remove: (id: string) => command<boolean>("provider_account_delete", { id }),
  refresh: (id: string) =>
    command<ProviderAccountView>("provider_account_refresh", { id }),
  activity: (id: string, lease: string, active: boolean) =>
    command<boolean>("provider_account_activity", { id, lease, active }),
  usage: (id: string, range: AccountUsageRange) =>
    command<AccountUsage>("provider_account_usage", { id, range }),
};
