import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { providerAccountsApi } from "@/lib/api/providerAccounts";

export function ProviderSessionSources({ harness, sessionId }: { harness: string; sessionId: string }) {
  const { i18n } = useTranslation();
  const zh = i18n.language.startsWith("zh");
  const accounts = useQuery({ queryKey: ["provider-accounts"], queryFn: providerAccountsApi.list, staleTime: 30_000 });
  const custom = accounts.data?.filter(v => v.account.custom);
  const sources = useQuery({
    queryKey: ["usage", "provider-session-sources", harness, sessionId, custom?.map(v => [v.account.id, v.account.name, v.account.links])],
    queryFn: () => providerAccountsApi.sessionSources(harness, sessionId),
    enabled: !!custom?.length,
    staleTime: 30_000,
  });
  const rows = sources.data ?? [];
  if (!rows.some(v => v.accountId)) return null;
  const names = [...new Set(rows.map(v => v.providerName ?? v.providerId ?? (zh ? "未知" : "Unknown")))];
  return <details className="border-b border-border px-5 py-2 text-caption text-fg-2 bg-app shrink-0">
    <summary className="cursor-pointer truncate" title={names.join(" · ")}>{zh ? "供应商来源" : "Provider sources"} · {names.join(" · ")}</summary>
    <div className="mt-2 space-y-1 max-h-40 overflow-auto">
      {rows.map((row, index) => <p key={index} className="break-words">
        <span className="font-medium">{row.providerName ?? row.providerId ?? (zh ? "未知" : "Unknown")}</span>{" · "}
        <span className="font-mono">{row.model}</span>{" · "}{row.requests} {zh ? "次" : "requests"}
        {row.providerName && <span className="text-fg-3"> · {zh ? "关联 ID" : "Linked ID"}: {row.providerId}</span>}
      </p>)}
    </div>
  </details>;
}
