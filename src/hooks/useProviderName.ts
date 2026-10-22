import { useQuery } from "@tanstack/react-query";
import { providerAccountsApi } from "@/lib/api/providerAccounts";
import { customProviderName } from "@/lib/providerAccounts";

export function useProviderName() {
  const { data } = useQuery({ queryKey: ["provider-accounts"], queryFn: providerAccountsApi.list, staleTime: 30_000 });
  return (provider: string | undefined, harness: string) => customProviderName(data ?? [], provider, harness);
}
