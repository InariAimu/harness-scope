import { providerAccountsApi } from "@/lib/api/providerAccounts";
import type { ProviderAccountView } from "@/types/providerAccounts";

export type CodexReferenceArgs = Parameters<
  typeof providerAccountsApi.codexEstimate
>[0];

export function codexReferenceQuery(
  view: ProviderAccountView,
  args: CodexReferenceArgs,
  revision: string | undefined,
) {
  return {
    queryKey: [
      "usage",
      "codex-reference",
      args,
      revision,
      view.account.links,
      view.snapshot?.fetchedAt,
    ],
    queryFn: () => providerAccountsApi.codexEstimate(args),
    staleTime: 15_000,
    refetchInterval: 30_000,
  };
}
