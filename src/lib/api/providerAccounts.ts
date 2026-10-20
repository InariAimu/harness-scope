import { command } from "@/lib/client";
import type {
  AccountRoute,
  AccountSave,
  AccountUsage,
  ProviderAccountView,
} from "@/types/providerAccounts";

export const providerAccountsApi = {
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
