import { useQuery } from "@tanstack/react-query";
import { providerAccountsApi } from "@/lib/api/providerAccounts";

/** Reads local snapshots; the backend owns the per-account provider polling. */
export function useProviderAccounts(enabled = true) {
  return useQuery({
    queryKey: ["provider-accounts"],
    queryFn: providerAccountsApi.list,
    staleTime: 30_000,
    enabled,
    refetchInterval: enabled ? 2_000 : false,
  });
}

export function useProviderSidebarOrder(enabled = true) {
  return useQuery({
    queryKey: ["provider-sidebar-order"],
    queryFn: providerAccountsApi.sidebarOrder,
    staleTime: 30_000,
    enabled,
  });
}
