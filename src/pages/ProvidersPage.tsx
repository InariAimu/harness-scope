import { useUsageNumberFormat } from "@/hooks/useUsageNumberFormat";
import { OptionSelect } from "@/components/ui/option-select";
import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  Wallet,
  Plus,
  RefreshCw,
  Settings2,
  KeyRound,
  Link2,
} from "lucide-react";
import type { Snapshot } from "@/App";
import type {
  AccountProvider,
  AccountUsageRange,
  ProviderAccountView,
  QuotaWindow,
} from "@/types/providerAccounts";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { Button } from "@/components/ui/button";
import { appShortName } from "@/components/usage/RequestLogTable";
import {
  getLocaleFromLanguage,
} from "@/components/usage/format";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import { useProviderActivity } from "@/hooks/useProviderActivity";
import { providerAccountsApi as api } from "@/lib/api/providerAccounts";
import {
  ACCOUNT_PROVIDER_NAMES,
  accountError,
  nativeMoney,
  quotaDisplay,
  routeKey,
  accountCanRefresh,
  accountProviderName,
  isCustomProvider,
} from "@/lib/providerAccounts";
import { ProviderAccountEditor } from "./ProviderAccountEditor";
import { GoUsagePanel } from "@/components/providers/GoUsagePanel";
import { CodexReferencePanel } from "@/components/providers/CodexReferencePanel";
import { codexWindowLabel } from "@/lib/codexSubscription";
import { cn } from "@/lib/utils";
import { useProviderAccounts } from "@/lib/query/providerAccounts";
import { CustomUsagePanel } from "@/components/providers/CustomUsagePanel";
import { ProviderHistoryPanel } from "@/components/providers/ProviderHistoryPanel";
import { IconPickerPopover } from "@/components/icons/IconPickerPopover";
import { ProviderGlyph } from "@/components/icons/ProviderGlyph";
import { defaultProviderIcon } from "@/lib/entityIcons";

function QuotaCard({
  window,
  label,
  now,
  zh,
  date,
}: {
  window?: QuotaWindow;
  label: string;
  now: number;
  zh: boolean;
  date: (v: number) => string;
}) {
  const view = window ? quotaDisplay(window, now) : null;
  return (
    <section className="scope-card !p-4 space-y-2.5">
      <div className="flex justify-between gap-2 text-caption text-fg-2">
        <span>{label}</span>
        <span>{zh ? "剩余" : "Remaining"}</span>
      </div>
      <div className="flex items-baseline gap-2">
        <strong className="text-2xl font-semibold tabular-nums">
          {view && !view.expired
            ? `${Number(view.remaining.toFixed(1))}%`
            : "—"}
        </strong>
        {view?.expired && (
          <span className="text-caption text-fg-3">
            {zh ? "等待刷新" : "Awaiting refresh"}
          </span>
        )}
      </div>
      <div
        role="progressbar"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={view && !view.expired ? view.remaining : undefined}
        className="h-1.5 overflow-hidden rounded-full bg-subtle"
      >
        <div
          className={cn(
            "h-full rounded-full transition-[width] bg-action",
            view?.limited && "!bg-danger",
          )}
          style={{ width: !view || view.expired ? "0%" : `${view.remaining}%` }}
        />
      </div>
      <p
        className="text-caption text-fg-3 min-h-4"
        title={window ? date(window.resetsAt) : undefined}
      >
        {window
          ? `${zh ? "重置" : "Resets"} ${date(window.resetsAt)}`
          : zh
            ? "查询后显示官方额度"
            : "Fetch to see provider limits"}
      </p>
      {window?.status === "unknown" && (
        <p className="text-caption text-fg-3">
          {zh ? "供应商返回未知状态" : "Unknown provider status"}
        </p>
      )}
    </section>
  );
}
export function ProvidersPage({
  snapshot,
  accountToSelect,
  onAccountSelected,
}: {
  snapshot: Snapshot;
  accountToSelect?: string;
  onAccountSelected: () => void;
}) {
  const { i18n } = useTranslation();
  const formatQuantity = useUsageNumberFormat();
  const zh = i18n.language.startsWith("zh");
  const l = (a: string, b: string) => (zh ? a : b);
  const locale = getLocaleFromLanguage(i18n.language);
  const client = useQueryClient();
  const { costMoney } = useMoneyFormat();
  const [provider, setProvider] = useState<AccountProvider | "all">("all");
  const [id, setId] = useState("");
  const [editing, setEditing] = useState<
    ProviderAccountView | null | undefined
  >();
  const [range, setRange] = useState<AccountUsageRange>("month");
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now() / 1000), 30_000);
    return () => clearInterval(timer);
  }, []);
  const list = useProviderAccounts();
  const accounts = list.data ?? [];
  useEffect(() => {
    if (!accountToSelect || !list.data) return;
    const target = list.data.find((v) => v.account.id === accountToSelect);
    if (target) {
      setProvider(target.account.provider);
      setId(target.account.id);
    }
    onAccountSelected();
  }, [accountToSelect, list.data, onAccountSelected]);
  const visible = accounts.filter(
    (v) => provider === "all" || v.account.provider === provider,
  );
  const selected = visible.find((v) => v.account.id === id) ?? visible[0];
  const account = selected?.account;
  useProviderActivity(account?.id);
  const updateView = (view: ProviderAccountView) => {
    client.setQueryData<ProviderAccountView[]>(
      ["provider-accounts"],
      (previous) =>
        (previous ?? []).map((v) =>
          v.account.id === view.account.id ? view : v,
        ),
    );
    void client.invalidateQueries({ queryKey: ["provider-history", view.account.id] });
  };
  const remote = useMutation({
    mutationFn: api.refresh,
    onSuccess: updateView,
  });
  const refreshing =
    !!selected?.refreshing ||
    (remote.isPending && remote.variables === account?.id);
  const supportsWeekly =
    account?.provider === "openai-codex" || account?.provider === "opencode-go";
  const weeklyWindow = selected?.weeklyUsageWindow;
  const weeklyReady =
    !!weeklyWindow && weeklyWindow.startDate <= now && weeklyWindow.resetsAt > now;
  const effectiveRange =
    (range === "subscription" && !account?.subscription.periodStart) ||
    (range === "weekly" && !supportsWeekly)
      ? "month"
      : range;
  const usageRanges: {
    id: AccountUsageRange;
    label: string;
    disabled?: boolean;
  }[] = [
    { id: "today", label: l("今天", "Today") },
    { id: "month", label: l("本月", "This month") },
    {
      id: "subscription",
      label: l("订阅周期", "Subscription period"),
      disabled: !account?.subscription.periodStart,
    },
  ];
  if (supportsWeekly) {
    usageRanges.push({
      id: "weekly",
      label: l("本周额度周期", "Weekly quota period"),
      disabled: !weeklyReady,
    });
  }
  const usage = useQuery({
    queryKey: [
      "usage",
      "provider-account",
      account?.id,
      account?.links,
      account?.subscription.periodStart,
      account?.subscription.renewalDate,
      effectiveRange,
      ...(effectiveRange === "weekly"
        ? [weeklyWindow?.startDate, weeklyWindow?.resetsAt, weeklyWindow?.bucket]
        : []),
      snapshot.config.timezone,
    ],
    queryFn: () => api.usage(account!.id, effectiveRange),
    enabled: !!account && (effectiveRange !== "weekly" || weeklyReady),
    staleTime: 30_000,
    refetchInterval: 60_000,
  });
  const date = (value: number) =>
    new Date(value * 1000).toLocaleString(locale, {
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    });
  const data = selected?.snapshot?.data;
  const periodDate = (value: number) =>
    new Date(value * 1000).toLocaleString(locale, {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      timeZone: snapshot.config.timezone,
    });
  const sub = account?.subscription;
  const error =
    remote.error && remote.variables === account?.id
      ? String(remote.error)
      : selected?.error;
  const monthlyReset =
    data?.kind === "quota"
      ? data.windows.find((v) => v.id === "monthly")?.resetsAt
      : undefined;
  const intervalText = (seconds: number) =>
    seconds === 0 ? l("关闭", "Off") : `${seconds}${l(" 秒", "s")}`;
  return (
    <>
      <AppPageHeader
        title={l("供应商", "Providers")}
        icon={<Wallet size={18} />}
        actions={
          <>
            <Button
              variant="neutral"
              disabled={
                !selected ||
                !accountCanRefresh(selected) ||
                refreshing
              }
              onClick={() => account && remote.mutate(account.id)}
            >
              <RefreshCw
                size={14}
                className={refreshing ? "animate-spin" : ""}
              />
              {l("刷新", "Refresh")}
            </Button>
            <Button
              variant="neutral"
              disabled={!selected}
              onClick={() => setEditing(selected)}
            >
              <Settings2 size={14} />
              {l("编辑", "Edit")}
            </Button>
            <Button onClick={() => setEditing(null)}>
              <Plus size={14} />
              {l("添加账号", "Add account")}
            </Button>
          </>
        }
      />
      <div className="scope-page space-y-4">
        <div className="flex flex-wrap items-center gap-2.5">
          <label className="text-caption text-fg-2" htmlFor="account-provider">
            {l("供应商", "Provider")}
          </label>
          <OptionSelect
            id="account-provider"
            className="scope-select"
            value={provider}
            onValueChange={(value) => {
              setProvider(value as AccountProvider | "all");
              setId("");
            }}
          >
            <option value="all">{l("全部", "All")}</option>
            {Object.entries(ACCOUNT_PROVIDER_NAMES).map(([v, label]) => (
              <option key={v} value={v}>
                {label}
              </option>
            ))}
            {accounts.filter(v => isCustomProvider(v.account.provider)).map(v => <option key={v.account.provider} value={v.account.provider}>{v.account.name}</option>)}
          </OptionSelect>
          <label
            className="text-caption text-fg-2 ms-1"
            htmlFor="provider-account"
          >
            {l("账号", "Account")}
          </label>
          <OptionSelect
            id="provider-account"
            className="scope-select max-w-64"
            value={account?.id ?? ""}
            disabled={!visible.length}
            onValueChange={(value) => setId(value)}
          >
            {!visible.length && (
              <option value="">{l("暂无账号", "No accounts")}</option>
            )}
            {visible.map((v) => (
              <option key={v.account.id} value={v.account.id}>
                {v.account.name}
              </option>
            ))}
          </OptionSelect>
          <span className="ms-auto text-caption text-fg-3">
            {l("账号与凭据仅本机保存", "Accounts and credentials stay on this device")}
          </span>
        </div>
        {list.isError && (
          <section role="alert" className="scope-card text-danger-text">
            {l("账号读取失败。", "Could not load accounts.")}{" "}
            <Button variant="neutral" onClick={() => void list.refetch()}>
              {l("重试", "Retry")}
            </Button>
          </section>
        )}
        {list.isLoading ? (
          <section className="scope-card text-fg-3">
            {l("正在读取账号…", "Loading accounts…")}
          </section>
        ) : !selected ? (
          <section className="scope-card py-12 flex flex-col items-center text-center gap-3">
            <Wallet size={30} className="text-fg-3" />
            <h2 className="text-section">
              {accounts.length
                ? l("此供应商暂无账号", "No accounts for this provider")
                : l(
                    "添加你的第一个供应商账号",
                    "Add your first provider account",
                  )}
            </h2>
            <p className="max-w-md text-body text-fg-2">
              {l(
                "查看 DeepSeek 余额、OpenCode Go 或 OpenAI／Codex 订阅额度，并关联所有已同步设备的用量。",
                "View DeepSeek balances, OpenCode Go or OpenAI/Codex subscription limits, and link usage from all synced devices.",
              )}
            </p>
            <Button variant="neutral" onClick={() => setEditing(null)}>
              <Plus size={14} />
              {l("添加账号", "Add account")}
            </Button>
          </section>
        ) : (
          <>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-caption text-fg-3">
              <span className="inline-flex items-center gap-2 text-fg-1 font-medium">
                <IconPickerPopover key={account!.id} target={{ kind: "account", id: account!.id }} name={account!.name}
                  fallback={defaultProviderIcon(account!.provider)} className="inline-flex h-7 w-7 items-center justify-center">
                  <ProviderGlyph account={account!} size={20} />
                </IconPickerPopover>
                {accountProviderName(account!.provider, accounts, zh)}
              </span>
              <span className="inline-flex items-center gap-1">
                <KeyRound size={12} />
                {account!.custom && !account!.custom.enabled ? l("用量查询已关闭", "Usage query disabled") : account!.provider === "openai-codex"
                  ? l("使用本机 Codex 登录", "Local Codex login")
                  : selected.keyConfigured
                    ? l("Key 已保存", "Key saved")
                    : l("未配置 Key", "No key configured")}
              </span>
              <span>
                {selected.snapshot
                  ? `${l("上次成功", "Last success")} ${date(selected.snapshot.fetchedAt)}`
                  : l("尚未查询", "Not fetched yet")}
              </span>
              {refreshing && (
                <span>{l("正在查询官方数据…", "Fetching provider data…")}</span>
              )}
              <span
                title={l(
                  "窗口有焦点且停留在此账号时使用前台间隔；其他情况使用后台间隔。",
                  "The foreground interval applies while this account page has focus; otherwise the background interval applies.",
                )}
              >
                {l("自动刷新", "Auto refresh")}: {l("前台", "Foreground")}{" "}
                {intervalText(account!.refresh.foregroundSeconds)} ·{" "}
                {l("后台", "Background")}{" "}
                {intervalText(account!.refresh.backgroundSeconds)}
              </span>
            </div>
            {error && (
              <div
                role="alert"
                className="rounded-control bg-danger-soft px-3 py-2 text-caption text-danger-text"
              >
                {accountError(error, zh)}{" "}
                {selected.snapshot &&
                  l(
                    "下方保留上次成功的数据。",
                    "Showing the last successful data below.",
                  )}
              </div>
            )}
            {account!.provider !== "openai-codex" &&
              !account!.custom &&
              !selected.keyConfigured && (
                <div className="rounded-control border border-border bg-subtle px-3 py-2 text-caption text-fg-2">
                  {l(
                    "编辑账号并保存 API Key 后可查询官方余额／额度。本机用量不受影响。",
                    "Save an API key to fetch provider balances or limits. Local usage remains available.",
                  )}
                </div>
              )}
            {account!.custom ? (
              data?.kind === "customUsage" ? <CustomUsagePanel data={data} zh={zh} locale={locale} timezone={snapshot.config.timezone} /> :
              <section className="scope-card !p-4 text-caption text-fg-3">{account!.custom.enabled
                ? l("刷新后显示自定义查询的余额或额度。", "Refresh to see balances or quotas from your custom query.")
                : l("已用于 Provider ID 归属，可在编辑中启用用量查询。", "Provider ID links are active. Enable usage queries in Edit when needed.")}</section>
            ) : account!.provider === "openai-codex" ? (
              <div className="space-y-3">
                {data?.kind === "codexQuota" ? (
                  data.buckets.map((bucket) => (
                    <div key={bucket.id} className="space-y-2">
                      <p className="text-caption text-fg-2">
                        {l("官方额度", "Provider limits")} ·{" "}
                        {bucket.name ?? bucket.id} ·{" "}
                        {l("套餐标识", "Plan identifier")}:{" "}
                        {bucket.planType ?? data.planType ?? "—"}
                      </p>
                      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                        {bucket.windows.map((window) => (
                          <QuotaCard
                            key={window.id}
                            window={window}
                            label={codexWindowLabel(window.durationMinutes, zh)}
                            now={now}
                            zh={zh}
                            date={date}
                          />
                        ))}
                      </div>
                      {!bucket.windows.length && (
                        <p className="text-caption text-fg-3">
                          {l(
                            "接口未提供额度窗口，不推断为无限可用。",
                            "No quota window returned; unlimited use is not assumed.",
                          )}
                        </p>
                      )}
                      {bucket.credits && (
                        <p className="text-caption text-fg-3">
                          Credits: {bucket.credits.balance ?? "—"}
                          {bucket.credits.unlimited
                            ? l(
                                "（接口标记 unlimited）",
                                " (reported unlimited)",
                              )
                            : ""}
                        </p>
                      )}
                    </div>
                  ))
                ) : (
                  <section className="scope-card !p-4 text-caption text-fg-3">
                    {l(
                      "刷新后显示本机 Codex 登录对应的官方额度与重置时间。",
                      "Refresh to read limits and reset times for the local Codex login.",
                    )}
                  </section>
                )}
              </div>
            ) : account!.provider === "opencode-go" ? (
              <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                {(["rolling", "weekly", "monthly"] as const).map((id, i) => (
                  <QuotaCard
                    key={id}
                    label={
                      [
                        l("5 小时窗口", "5-hour window"),
                        l("每周窗口", "Weekly window"),
                        l("每月窗口", "Monthly window"),
                      ][i]
                    }
                    window={
                      data?.kind === "quota"
                        ? data.windows.find((v) => v.id === id)
                        : undefined
                    }
                    now={now}
                    zh={zh}
                    date={date}
                  />
                ))}
              </div>
            ) : (
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                {data?.kind === "balance" ? (
                  data.balances.map((balance) => (
                    <section
                      key={balance.currency}
                      className="scope-card !p-4 space-y-2"
                    >
                      <div className="flex justify-between text-caption text-fg-2">
                        <span>{l("官方可用余额", "Provider balance")}</span>
                        <span>{balance.currency}</span>
                      </div>
                      <strong
                        className="block text-2xl font-semibold tabular-nums"
                        title={`${balance.total} ${balance.currency}`}
                      >
                        {nativeMoney(balance.total, balance.currency, locale)}
                      </strong>
                      <div className="flex flex-wrap gap-x-5 gap-y-1 text-caption text-fg-3">
                        <span>
                          {l("充值", "Purchased")}{" "}
                          {nativeMoney(
                            balance.toppedUp,
                            balance.currency,
                            locale,
                          )}
                        </span>
                        <span>
                          {l("赠金", "Granted")}{" "}
                          {nativeMoney(
                            balance.granted,
                            balance.currency,
                            locale,
                          )}
                        </span>
                      </div>
                      {!data.available && (
                        <p className="text-caption text-danger-text">
                          {l(
                            "供应商报告余额不可用",
                            "Provider reports no available balance",
                          )}
                        </p>
                      )}
                    </section>
                  ))
                ) : (
                  <section className="scope-card !p-4 space-y-2">
                    <p className="text-caption text-fg-2">
                      {l("官方可用余额", "Provider balance")}
                    </p>
                    <strong className="block text-2xl">—</strong>
                    <p className="text-caption text-fg-3">
                      {l(
                        "查询后按官方原币显示充值与赠金",
                        "Fetch to see purchased and granted balances in their original currency",
                      )}
                    </p>
                  </section>
                )}
              </div>
            )}
            <ProviderHistoryPanel key={`history:${account!.id}`} view={selected} zh={zh} locale={locale} timezone={snapshot.config.timezone} usageVersion={snapshot.usageVersion} />
            <section className="scope-card !p-4">
              <div className="flex items-center justify-between mb-3">
                <h2 className="font-semibold">
                  {l("订阅信息", "Subscription")}
                </h2>
                <span className="text-caption text-fg-3">
                  {l("手动记录", "Manually entered")}
                </span>
              </div>
              <dl className="grid grid-cols-2 sm:grid-cols-4 gap-3 text-body">
                {[
                  [l("套餐", "Plan"), sub!.planName || "—"],
                  [
                    l("每月费用", "Monthly fee"),
                    nativeMoney(sub!.monthlyFee, sub!.currency, locale),
                  ],
                  [l("本期开始", "Period start"), sub!.periodStart || "—"],
                  [l("下次续费", "Next renewal"), sub!.renewalDate || "—"],
                ].map(([label, value]) => (
                  <div key={label}>
                    <dt className="text-caption text-fg-3 mb-1">{label}</dt>
                    <dd className="font-medium tabular-nums break-words">
                      {value}
                    </dd>
                  </div>
                ))}
              </dl>
              {account!.provider === "opencode-go" && (
                <div className="mt-3 border-t border-border pt-3 space-y-1 text-caption text-fg-3">
                  {monthlyReset !== undefined && (
                    <p>
                      {l("官方月额度重置", "Provider monthly quota resets")}:{" "}
                      <span className="tabular-nums text-fg-2">
                        {date(monthlyReset)}
                      </span>
                    </p>
                  )}
                  <p>
                    {l(
                      "套餐、月费和续费日期保留手动填写；官方额度重置时间不代表续费时间。",
                      "Plan, monthly fee and renewal date remain manually entered. A quota reset is not a billing renewal.",
                    )}
                  </p>
                </div>
              )}
            </section>
            {account!.provider === "opencode-go" && (
              <GoUsagePanel
                key={account!.id}
                view={selected}
                zh={zh}
                locale={locale}
              />
            )}
            {account!.provider === "openai-codex" && (
              <CodexReferencePanel
                key={account!.id}
                view={selected}
                zh={zh}
                locale={locale}
                timeZone={snapshot.config.timezone}
              />
            )}
            <section
              aria-label={l("关联设备用量", "Linked device usage")}
              className="scope-card !p-4 space-y-3"
            >
              <div className="flex flex-wrap items-center justify-between gap-2">
                <h2 className="font-semibold">
                  {l("关联设备用量", "Linked device usage")}{" "}
                  <span className="text-caption font-normal text-fg-3">
                    {l("日志估算", "From usage logs")}
                  </span>
                </h2>
                <div className="flex flex-wrap gap-1">
                  {usageRanges.map(({ id, label, disabled }) => (
                    <Button
                      key={id}
                      size="compact"
                      variant={effectiveRange === id ? "neutral" : "quiet"}
                      aria-pressed={effectiveRange === id}
                      disabled={disabled}
                      onClick={() => setRange(id)}
                    >
                      {label}
                    </Button>
                  ))}
                </div>
              </div>
              {supportsWeekly && !weeklyReady && (
                <p className="text-caption text-fg-3">
                  {l(
                    "暂无有效的官方周额度窗口，请先刷新官方额度。",
                    "No active official weekly quota window. Refresh provider limits first.",
                  )}
                </p>
              )}
              {effectiveRange === "weekly" &&
                weeklyReady &&
                usage.data?.weeklyUsageWindow && (
                  <p className="text-caption text-fg-3 tabular-nums">
                    {l("官方周额度周期", "Official weekly quota period")}: {" "}
                    {periodDate(usage.data.weeklyUsageWindow.startDate)} → {" "}
                    {periodDate(usage.data.weeklyUsageWindow.resetsAt)}
                    {usage.data.weeklyUsageWindow.bucket &&
                      ` · ${usage.data.weeklyUsageWindow.bucket}`}
                    {` · ${snapshot.config.timezone}`}
                    <br />
                    {l("已统计至", "Usage through")} {periodDate(usage.data.endDate)}
                  </p>
                )}
              <div className="flex flex-wrap gap-1.5">
                {account!.links.length ? (
                  account!.links.map((link) => (
                    <span
                      key={routeKey(link)}
                      className="inline-flex max-w-full items-center gap-1 rounded-control border border-border px-2 py-0.5 text-caption text-fg-2"
                    >
                      <Link2 size={11} />
                      <span className="truncate">
                        {appShortName(link.harness)} / {link.provider}
                      </span>
                    </span>
                  ))
                ) : (
                  <p className="text-caption text-fg-3">
                    {l(
                      "尚未关联路由，编辑账号后选择已采集或同步日志中的供应商。",
                      "No routes linked. Edit this account to select providers from collected or synced logs.",
                    )}
                  </p>
                )}
              </div>
              {effectiveRange === "weekly" && !weeklyReady ? null : usage.isError ? (
                <p role="alert" className="text-caption text-danger-text">
                  {effectiveRange === "weekly"
                    ? l(
                        "官方周额度周期读取失败，请刷新官方额度后重试。",
                        "Could not read the weekly quota period. Refresh provider limits and retry.",
                      )
                    : l("关联用量读取失败。", "Could not load linked usage.")}
                  <Button
                    size="compact"
                    variant="quiet"
                    onClick={() => void usage.refetch()}
                  >
                    {l("重试", "Retry")}
                  </Button>
                </p>
              ) : usage.isLoading ? (
                <p className="text-caption text-fg-3">
                  {l("正在读取关联用量…", "Loading linked usage…")}
                </p>
              ) : (
                usage.data?.linked && (
                  <>
                    <div className="grid grid-cols-3 gap-3 border-y border-border py-3">
                      {[
                        [
                          l("请求数", "Requests"),
                          formatQuantity(usage.data.summary.totalRequests),
                        ],
                        [
                          l("Tokens", "Tokens"),
                          formatQuantity(usage.data.summary.realTotalTokens),
                        ],
                        [
                          l("按价表折算", "Priced usage"),
                          costMoney(
                            usage.data.summary.totalCost,
                            4,
                            usage.data.summary.costConverted,
                          ),
                        ],
                      ].map(([label, value]) => (
                        <div key={label}>
                          <p className="text-caption text-fg-3 mb-1">{label}</p>
                          <strong className="text-strong tabular-nums">
                            {value}
                          </strong>
                        </div>
                      ))}
                    </div>
                    {!!usage.data.summary.unpricedCount && (
                      <p className="text-caption text-fg-3">
                        {l(
                          `${usage.data.summary.unpricedCount} 条请求尚未定价，未计入折算金额。`,
                          `${usage.data.summary.unpricedCount} unpriced requests are excluded from the monetary estimate.`,
                        )}
                      </p>
                    )}
                    {!!usage.data.models.length && (
                      <div className="overflow-x-auto">
                        <table className="w-full text-caption">
                          <thead className="text-fg-3">
                            <tr>
                              <th className="text-start font-normal pb-2">
                                {l("模型", "Model")}
                              </th>
                              <th className="text-end font-normal pb-2">
                                {l("请求", "Requests")}
                              </th>
                              <th className="text-end font-normal pb-2">
                                Tokens
                              </th>
                              <th className="text-end font-normal pb-2">
                                {l("折算金额", "Priced usage")}
                              </th>
                            </tr>
                          </thead>
                          <tbody>
                            {usage.data.models.map((v) => (
                              <tr
                                key={v.model}
                                className="border-t border-border"
                              >
                                <td
                                  className="py-2 max-w-52 truncate font-mono"
                                  title={v.model}
                                >
                                  {v.model}
                                </td>
                                <td className="text-end tabular-nums">
                                  {formatQuantity(v.summary.totalRequests)}
                                </td>
                                <td className="text-end tabular-nums">
                                  {formatQuantity(v.summary.realTotalTokens)}
                                </td>
                                <td className="text-end tabular-nums">
                                  {costMoney(
                                    v.summary.totalCost,
                                    4,
                                    v.summary.costConverted,
                                  )}
                                </td>
                              </tr>
                            ))}
                          </tbody>
                        </table>
                      </div>
                    )}
                  </>
                )
              )}
              <p className="text-caption text-fg-3">
                {l(
                  "按共用订阅汇总所有已同步设备的关联路由，不能区分同一路由的不同账号或 Key；尚未同步的用量不会计入。折算金额不代表实际扣款或套餐剩余。",
                  "Includes linked routes from all synced devices, assuming a shared subscription. Accounts or keys on the same route cannot be distinguished; unsynced usage is absent. Priced usage is not an invoice or remaining allowance.",
                )}{" "}
                {usage.data &&
                  (effectiveRange !== "weekly" || weeklyReady) &&
                  `${l("统计时区", "Time zone")}: ${usage.data.timezone}`}
              </p>
            </section>
          </>
        )}
      </div>
      {editing !== undefined && (
        <ProviderAccountEditor
          key={editing?.account.id ?? "new"}
          initial={editing ?? undefined}
          provider={provider === "all" ? "deepseek" : provider}
          accounts={accounts}
          timezone={snapshot.config.timezone}
          onClose={() => setEditing(undefined)}
          onSaved={(view) => {
            client.setQueryData<ProviderAccountView[]>(
              ["provider-accounts"],
              (previous) => [
                ...(previous ?? []).filter(
                  (v) => v.account.id !== view.account.id,
                ),
                view,
              ],
            );
            setId(view.account.id);
            setProvider("all");
            setEditing(undefined);
            void client.invalidateQueries({
              queryKey: ["usage", "provider-account", view.account.id],
            });
            void client.invalidateQueries({
              queryKey: ["usage", "provider-estimate", view.account.id],
            });
          }}
        />
      )}
    </>
  );
}
