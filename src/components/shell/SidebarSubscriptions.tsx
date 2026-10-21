import { useEffect, useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { AlertCircle, ChevronDown, RefreshCw, Wallet } from "lucide-react";
import { providerAccountsApi as api } from "@/lib/api/providerAccounts";
import { useProviderAccounts } from "@/lib/query/providerAccounts";
import { accountError, nativeMoney, quotaDisplay } from "@/lib/providerAccounts";
import { quotaCountdown, quotaWindowShortLabel, snapshotAge } from "@/lib/sidebarSubscriptions";
import { toast } from "@/lib/toast";
import type { ProviderAccountView, QuotaWindow } from "@/types/providerAccounts";

const EXPANDED_KEY = "harness-scope.sidebar-subscriptions-expanded";

function QuotaRow({ window, now, zh }: { window: QuotaWindow; now: number; zh: boolean }) {
  const display = quotaDisplay(window, now);
  const ready = !display.expired && window.status !== "unknown";
  const label = quotaWindowShortLabel(window, zh);
  const countdown = quotaCountdown(window.resetsAt, now);
  const percent = Number(display.remaining.toFixed(1));
  const description = !ready
    ? zh ? "等待刷新或供应商确认额度" : "Awaiting refresh or provider confirmation"
    : `${zh ? "距重置" : "Resets in"} ${countdown} · ${zh ? "剩余" : "Remaining"} ${percent}%`;
  return (
    <span className={`scope-subscription-quota ${ready && percent <= 10 ? "is-low" : ""}`} title={`${label} · ${description}`}>
      <span className="text-fg-2">{label}</span>
      <span className="text-fg-3">{countdown}</span>
      <span
        role="progressbar"
        aria-label={`${label} ${zh ? "剩余额度" : "remaining quota"}`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={ready ? percent : undefined}
        aria-valuetext={ready ? `${percent}%` : description}
        className="scope-subscription-progress"
      >
        <span style={{ width: ready ? `${display.remaining}%` : "0%" }} />
      </span>
      <span className="scope-subscription-percent">{ready ? `${percent}%` : "—"}</span>
    </span>
  );
}

function AccountSummary({ view, now, zh, locale, onOpen }: {
  view: ProviderAccountView;
  now: number;
  zh: boolean;
  locale: string;
  onOpen: (id: string) => void;
}) {
  const { account, snapshot } = view;
  const data = snapshot?.data;
  const age = snapshotAge(snapshot?.fetchedAt, now, zh);
  const groups = data?.kind === "codexQuota"
    ? data.buckets.map((bucket) => ({ id: bucket.id, name: bucket.name ?? bucket.id, windows: bucket.windows }))
    : data?.kind === "quota" ? [{ id: "quota", name: "", windows: data.windows }] : [];
  const details = [account.name,
    snapshot ? `${zh ? "最后成功" : "Last success"}: ${new Date(snapshot.fetchedAt * 1000).toLocaleString(locale)}` : age,
    view.error ? accountError(view.error, zh) : null,
    account.refresh.backgroundSeconds === 0 ? zh ? "后台自动刷新已关闭" : "Background refresh is off" : null,
  ].filter(Boolean).join("\n");
  return (
    <button type="button" className="scope-subscription-account" onClick={() => onOpen(account.id)}
      aria-label={`${zh ? "配置" : "Configure"} ${account.name}`} title={details}>
      <span className="scope-subscription-account-heading">
        <span className="scope-subscription-account-name">{account.name}</span>
        {view.error && <AlertCircle size={11} className="shrink-0 text-danger-text" aria-label={zh ? "刷新失败" : "Refresh failed"} />}
        <span className="scope-subscription-updated">{view.refreshing ? zh ? "刷新中" : "Refreshing" : age}</span>
      </span>
      {data?.kind === "balance" && data.balances.map((balance) => (
        <span key={balance.currency} className="scope-subscription-balance" title={nativeMoney(balance.total, balance.currency, locale)}>
          {nativeMoney(balance.total, balance.currency, locale)}
        </span>
      ))}
      {groups.map((group) => (
        <span key={group.id} className="block">
          {groups.length > 1 && <span className="block truncate text-[10px] leading-4 text-fg-3">{group.name}</span>}
          {group.windows.map((window) => <QuotaRow key={window.id} window={window} now={now} zh={zh} />)}
        </span>
      ))}
      {(!data || (data.kind !== "balance" && !groups.some((g) => g.windows.length))) && (
        <span className="block text-caption leading-5 text-fg-3">—</span>
      )}
    </button>
  );
}

export function SidebarSubscriptions({ collapsed, active, enabled, onNavigate, onAccountClick }: {
  collapsed: boolean;
  active: boolean;
  enabled: boolean;
  onNavigate: () => void;
  onAccountClick: (id: string) => void;
}) {
  const { i18n } = useTranslation();
  const zh = i18n.language.startsWith("zh");
  const client = useQueryClient();
  const list = useProviderAccounts(enabled && !collapsed);
  const accounts = list.data ?? [];
  const visible = accounts.filter((v) => v.account.showInSidebar !== false);
  const [now, setNow] = useState(() => Date.now() / 1000);
  const [expanded, setExpanded] = useState(() => {
    try { return localStorage.getItem(EXPANDED_KEY) !== "false"; } catch { return true; }
  });
  const [refreshing, setRefreshing] = useState(false);
  const refreshingRef = useRef(false);
  useEffect(() => {
    if (collapsed || !expanded) return;
    setNow(Date.now() / 1000);
    const timer = window.setInterval(() => setNow(Date.now() / 1000), 15_000);
    return () => clearInterval(timer);
  }, [collapsed, expanded]);

  const refreshAll = async () => {
    if (refreshingRef.current) return;
    // Visibility only affects this list; manual refresh includes hidden accounts.
    const targets = accounts.filter((v) => !v.refreshing && (v.account.provider === "openai-codex" || v.keyConfigured));
    if (!targets.length) {
      toast.info(zh ? "没有可刷新的账号，请先配置凭据或等待当前刷新完成。" : "Configure credentials or wait for the current refresh to finish.");
      return;
    }
    refreshingRef.current = true;
    setRefreshing(true);
    let failed = 0;
    try {
      // Match the backend's automatic concurrency limit without issuing 100 requests at once.
      for (let offset = 0; offset < targets.length; offset += 4) {
        const results = await Promise.allSettled(targets.slice(offset, offset + 4).map((v) => api.refresh(v.account.id)));
        for (const result of results) {
          if (result.status === "rejected") { failed += 1; continue; }
          const view = result.value;
          if (view.error) failed += 1;
          client.setQueryData<ProviderAccountView[]>(["provider-accounts"], (previous) =>
            previous?.map((v) => v.account.id === view.account.id ? view : v));
        }
      }
      if (failed) toast.warning(zh ? `${failed} 个账号刷新失败，请在供应商中查看详情。` : `${failed} account refreshes failed. See Providers for details.`);
      else toast.success(zh ? "账号刷新完成" : "Accounts refreshed");
    } finally {
      refreshingRef.current = false;
      setRefreshing(false);
      void client.invalidateQueries({ queryKey: ["provider-accounts"] });
    }
  };

  const showAccounts = !collapsed && visible.length > 0;
  const providerLabel = zh ? "供应商" : "Providers";
  const toggleLabel = expanded
    ? zh ? "收起供应商账号" : "Collapse provider accounts"
    : zh ? "展开供应商账号" : "Expand provider accounts";
  return (
    <div className="scope-provider-group">
      <div className={`scope-provider-row ${active ? "active" : ""} ${showAccounts ? "has-accounts" : ""}`}>
        <button type="button" className="scope-nav scope-provider-link"
          title={providerLabel} aria-label={providerLabel} onClick={onNavigate}>
          <Wallet size={16} className="shrink-0" aria-hidden="true" />
          {!collapsed && <span className="truncate">{providerLabel}</span>}
        </button>
        {showAccounts && expanded && <button type="button" className="scope-subscriptions-refresh" disabled={refreshing}
          aria-label={zh ? "刷新所有账号" : "Refresh all accounts"}
          title={zh ? "刷新所有账号（含未在左侧显示的账号）" : "Refresh all accounts, including hidden accounts"}
          onClick={() => void refreshAll()}>
          <RefreshCw size={14} className={refreshing ? "animate-spin" : ""} aria-hidden="true" />
        </button>}
        {showAccounts && <button type="button" className="scope-subscriptions-disclosure"
          aria-label={toggleLabel} title={toggleLabel} aria-expanded={expanded}
          aria-controls="sidebar-subscriptions-list" onClick={() => {
            const next = !expanded;
            setExpanded(next);
            try { localStorage.setItem(EXPANDED_KEY, String(next)); } catch { /* Optional preference. */ }
          }}>
          <ChevronDown size={14} className={expanded ? "" : "-rotate-90"} aria-hidden="true" />
        </button>}
      </div>
      {showAccounts && <div id="sidebar-subscriptions-list" hidden={!expanded} className="scope-subscriptions-list"
        role="region" aria-label={zh ? "供应商账号" : "Provider accounts"}>
        {visible.map((view) => (
          <div key={view.account.id} className="scope-subscription-item">
            <AccountSummary view={view} now={now} zh={zh}
              locale={i18n.language} onOpen={onAccountClick} />
          </div>
        ))}
      </div>}
    </div>
  );
}
