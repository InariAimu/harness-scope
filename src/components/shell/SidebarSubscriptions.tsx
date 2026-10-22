import { useEffect, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { AlertCircle, ArrowDown, ArrowDownUp, ArrowUp, Check, ChevronDown, RefreshCw, Wallet } from "lucide-react";
import { providerAccountsApi as api } from "@/lib/api/providerAccounts";
import { useProviderAccounts, useProviderSidebarOrder } from "@/lib/query/providerAccounts";
import { accountError, nativeMoney, quotaDisplay, accountCanRefresh, customUsageValue, customRemainingPercent } from "@/lib/providerAccounts";
import { quotaCountdown, quotaWindowShortLabel, snapshotAge } from "@/lib/sidebarSubscriptions";
import { moveProviderSidebarAccount, reconcileProviderSidebarOrder } from "@/lib/providerSidebarOrder";
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

function AccountSummary({ view, now, zh, locale, onOpen, sorting }: {
  view: ProviderAccountView;
  now: number;
  zh: boolean;
  locale: string;
  onOpen: (id: string) => void;
  sorting: boolean;
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
    <button type="button" className="scope-subscription-account" onClick={() => onOpen(account.id)} disabled={sorting}
      aria-label={`${zh ? "查看" : "View"} ${account.name}`} title={details}>
      <span className="scope-subscription-account-heading">
        <span className="scope-subscription-account-name">{account.name}</span>
        {view.error && <AlertCircle size={11} className="shrink-0 text-danger-text" aria-label={zh ? "刷新失败" : "Refresh failed"} />}
        {!sorting && <span className="scope-subscription-updated">{view.refreshing ? zh ? "刷新中" : "Refreshing" : age}</span>}
      </span>
      {data?.kind === "balance" && data.balances.map((balance) => (
        <span key={balance.currency} className="scope-subscription-balance" title={nativeMoney(balance.total, balance.currency, locale)}>
          {nativeMoney(balance.total, balance.currency, locale)}
        </span>
      ))}
      {data?.kind === "customUsage" && data.items.map(item => {
        const percent = customRemainingPercent(item, now);
        return <span key={item.id} className="block text-caption leading-5 text-fg-2 truncate" title={item.planName ?? item.id}>
          {data.items.length > 1 && `${item.planName ?? item.id} · `}{item.remaining != null ? customUsageValue(item.remaining, item.unit, locale) : percent != null ? `${Number(percent.toFixed(1))}%` : "—"}
        </span>;
      })}
      {groups.map((group) => (
        <span key={group.id} className="block">
          {groups.length > 1 && <span className="block truncate text-[10px] leading-4 text-fg-3">{group.name}</span>}
          {group.windows.map((window) => <QuotaRow key={window.id} window={window} now={now} zh={zh} />)}
        </span>
      ))}
      {(!data || (data.kind !== "balance" && data.kind !== "customUsage" && !groups.some((g) => g.windows.length))) && (
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
  const orderQuery = useProviderSidebarOrder(enabled && !collapsed);
  const accounts = list.data ?? [];
  const [draftOrder, setDraftOrder] = useState<string[] | null>(null);
  const [sortAnnouncement, setSortAnnouncement] = useState("");
  const sorting = draftOrder !== null;
  const orderedIds = reconcileProviderSidebarOrder(draftOrder ?? orderQuery.data ?? [], accounts.map(v => v.account.id));
  const accountsById = new Map(accounts.map(v => [v.account.id, v]));
  const visible = orderedIds.map(id => accountsById.get(id)!).filter(v => v.account.showInSidebar !== false);
  const saveOrder = useMutation({
    mutationFn: api.saveSidebarOrder,
    onMutate: () => client.cancelQueries({ queryKey: ["provider-sidebar-order"] }),
    onSuccess: ids => {
      client.setQueryData(["provider-sidebar-order"], ids);
      setDraftOrder(null);
      setSortAnnouncement(zh ? "供应商顺序已保存" : "Provider order saved");
    },
    onError: error => {
      toast.error(zh ? `保存排序失败：${String(error)}` : `Could not save order: ${String(error)}`);
      void client.invalidateQueries({ queryKey: ["provider-accounts"] });
    },
  });
  const moveAccount = (id: string, direction: -1 | 1) => {
    if (!sorting || saveOrder.isPending) return;
    const next = moveProviderSidebarAccount(orderedIds, visible.map(v => v.account.id), id, direction);
    setDraftOrder(next);
    const position = next.filter(id => accountsById.get(id)?.account.showInSidebar !== false).indexOf(id) + 1;
    const name = accountsById.get(id)!.account.name;
    setSortAnnouncement(zh ? `${name} 已移至第 ${position} 位，共 ${visible.length} 个账号` : `${name} moved to position ${position} of ${visible.length}`);
  };
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
    const targets = accounts.filter((v) => !v.refreshing && accountCanRefresh(v));
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
  const sortLabel = sorting
    ? saveOrder.isPending ? zh ? "正在保存排序" : "Saving order" : zh ? "确认供应商排序" : "Confirm provider order"
    : zh ? "调整供应商顺序" : "Reorder provider accounts";
  return (
    <div className={`scope-provider-group ${sorting ? "is-sorting" : ""}`}>
      <div className={`scope-provider-row ${active ? "active" : ""} ${showAccounts ? "has-accounts" : ""}`}>
        <button type="button" className="scope-nav scope-provider-link"
          title={providerLabel} aria-label={providerLabel} onClick={onNavigate}>
          <Wallet size={16} className="shrink-0" aria-hidden="true" />
          {!collapsed && <span className="truncate">{providerLabel}</span>}
        </button>
        {showAccounts && expanded && <button type="button" className="scope-subscriptions-sort"
          aria-label={sortLabel} title={sortLabel} aria-pressed={sorting}
          disabled={saveOrder.isPending || (!sorting && (visible.length < 2 || !orderQuery.data))}
          onClick={() => {
            if (sorting) saveOrder.mutate(orderedIds);
            else {
              setDraftOrder(orderedIds);
              setSortAnnouncement(zh ? "已进入排序模式，使用上下按钮调整账号位置" : "Reordering enabled. Use the up and down buttons to move accounts.");
            }
          }}>
          {sorting ? <Check size={14} aria-hidden="true" /> : <ArrowDownUp size={14} aria-hidden="true" />}
        </button>}
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
        {visible.map((view, index) => (
          <div key={view.account.id} className="scope-subscription-item">
            <AccountSummary view={view} now={now} zh={zh}
              locale={i18n.language} onOpen={onAccountClick} sorting={sorting} />
            {sorting && <div className="scope-subscription-sort-controls" role="group"
              aria-label={zh ? `调整 ${view.account.name} 的位置` : `Reorder ${view.account.name}`}>
              <button type="button" disabled={saveOrder.isPending || index === 0}
                aria-label={zh ? `上移 ${view.account.name}` : `Move ${view.account.name} up`}
                title={zh ? "上移" : "Move up"} onClick={() => moveAccount(view.account.id, -1)}>
                <ArrowUp size={12} aria-hidden="true" />
              </button>
              <button type="button" disabled={saveOrder.isPending || index === visible.length - 1}
                aria-label={zh ? `下移 ${view.account.name}` : `Move ${view.account.name} down`}
                title={zh ? "下移" : "Move down"} onClick={() => moveAccount(view.account.id, 1)}>
                <ArrowDown size={12} aria-hidden="true" />
              </button>
            </div>}
          </div>
        ))}
      </div>}
      <span className="sr-only" role="status" aria-live="polite">{sortAnnouncement}</span>
    </div>
  );
}
