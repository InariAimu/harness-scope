import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
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
  ProviderAccountView,
  QuotaWindow,
} from "@/types/providerAccounts";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { Button } from "@/components/ui/button";
import { appShortName } from "@/components/usage/RequestLogTable";
import {
  formatTokensCompact,
  getLocaleFromLanguage,
} from "@/components/usage/format";
import { useMoneyFormat } from "@/hooks/useMoneyFormat";
import { providerAccountsApi as api } from "@/lib/api/providerAccounts";
import {
  ACCOUNT_PROVIDER_NAMES,
  accountError,
  nativeMoney,
  quotaDisplay,
  routeKey,
} from "@/lib/providerAccounts";
import { ProviderAccountEditor } from "./ProviderAccountEditor";
import { cn } from "@/lib/utils";

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
export function ProvidersPage({ snapshot }: { snapshot: Snapshot }) {
  const { i18n } = useTranslation();
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
  const [range, setRange] = useState("month");
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now() / 1000), 30_000);
    return () => clearInterval(timer);
  }, []);
  const list = useQuery({
    queryKey: ["provider-accounts"],
    queryFn: api.list,
    staleTime: 30_000,
  });
  const accounts = list.data ?? [];
  const visible = accounts.filter(
    (v) => provider === "all" || v.account.provider === provider,
  );
  const selected = visible.find((v) => v.account.id === id) ?? visible[0];
  const account = selected?.account;
  const updateView = (view: ProviderAccountView) =>
    client.setQueryData<ProviderAccountView[]>(
      ["provider-accounts"],
      (previous) =>
        (previous ?? []).map((v) =>
          v.account.id === view.account.id ? view : v,
        ),
    );
  const remote = useQuery({
    queryKey: ["provider-account-refresh", account?.id],
    queryFn: async () => {
      const view = await api.refresh(account!.id);
      updateView(view);
      return view;
    },
    enabled: !!account && !!selected?.keyConfigured,
    staleTime: 60_000,
    refetchInterval: 60_000,
    retry: false,
    refetchOnWindowFocus: false,
  });
  const effectiveRange =
    range === "subscription" && !account?.subscription.periodStart
      ? "month"
      : range;
  const usage = useQuery({
    queryKey: [
      "usage",
      "provider-account",
      account?.id,
      effectiveRange,
      snapshot.config.timezone,
    ],
    queryFn: () => api.usage(account!.id, effectiveRange),
    enabled: !!account,
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
  const sub = account?.subscription;
  const error = remote.error ? String(remote.error) : selected?.error;
  return (
    <>
      <AppPageHeader
        title={l("供应商", "Providers")}
        icon={<Wallet size={18} />}
        actions={
          <>
            <Button
              variant="neutral"
              disabled={!selected?.keyConfigured || remote.isFetching}
              onClick={() => void remote.refetch()}
            >
              <RefreshCw
                size={14}
                className={remote.isFetching ? "animate-spin" : ""}
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
          <select
            id="account-provider"
            className="scope-select"
            value={provider}
            onChange={(e) => {
              setProvider(e.target.value as AccountProvider | "all");
              setId("");
            }}
          >
            <option value="all">{l("全部", "All")}</option>
            {Object.entries(ACCOUNT_PROVIDER_NAMES).map(([v, label]) => (
              <option key={v} value={v}>
                {label}
              </option>
            ))}
          </select>
          <label
            className="text-caption text-fg-2 ms-1"
            htmlFor="provider-account"
          >
            {l("账号", "Account")}
          </label>
          <select
            id="provider-account"
            className="scope-select max-w-64"
            value={account?.id ?? ""}
            disabled={!visible.length}
            onChange={(e) => setId(e.target.value)}
          >
            {!visible.length && (
              <option value="">{l("暂无账号", "No accounts")}</option>
            )}
            {visible.map((v) => (
              <option key={v.account.id} value={v.account.id}>
                {v.account.name}
              </option>
            ))}
          </select>
          <span className="ms-auto text-caption text-fg-3">
            {l("仅本机 · 不参与同步", "Device-local · excluded from sync")}
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
                : l("添加你的第一个供应商账号", "Add your first provider account")}
            </h2>
            <p className="max-w-md text-body text-fg-2">
              {l(
                "查看 DeepSeek 官方余额或 OpenCode Go 三窗口额度，并关联本机用量。也可以先填写订阅信息，稍后配置 Key。",
                "View DeepSeek balances or OpenCode Go limits and link local usage. You can add subscription details before configuring a key.",
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
              <span className="text-fg-1 font-medium">
                {ACCOUNT_PROVIDER_NAMES[account!.provider]}
              </span>
              <span className="inline-flex items-center gap-1">
                <KeyRound size={12} />
                {selected.keyConfigured
                  ? l("Key 已保存", "Key saved")
                  : l("未配置 Key", "No key configured")}
              </span>
              <span>
                {selected.snapshot
                  ? `${l("上次成功", "Last success")} ${date(selected.snapshot.fetchedAt)}`
                  : l("尚未查询", "Not fetched yet")}
              </span>
              {remote.isFetching && (
                <span>{l("正在查询官方数据…", "Fetching provider data…")}</span>
              )}
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
            {!selected.keyConfigured && (
              <div className="rounded-control border border-border bg-subtle px-3 py-2 text-caption text-fg-2">
                {l(
                  "编辑账号并保存 API Key 后可查询官方余额／额度。本机用量不受影响。",
                  "Save an API key to fetch provider balances or limits. Local usage remains available.",
                )}
              </div>
            )}
            {account!.provider === "opencode-go" ? (
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
            </section>
            <section className="scope-card !p-4 space-y-3">
              <div className="flex flex-wrap items-center justify-between gap-2">
                <h2 className="font-semibold">
                  {l("本机用量", "Local usage")}{" "}
                  <span className="text-caption font-normal text-fg-3">
                    {l("日志估算", "From local logs")}
                  </span>
                </h2>
                <div className="flex gap-1">
                  {[
                    ["today", l("今天", "Today")],
                    ["month", l("本月", "This month")],
                    ["subscription", l("本期", "Billing period")],
                  ].map(([id, label]) => (
                    <Button
                      key={id}
                      size="compact"
                      variant={effectiveRange === id ? "neutral" : "quiet"}
                      aria-pressed={effectiveRange === id}
                      disabled={id === "subscription" && !sub!.periodStart}
                      onClick={() => setRange(id)}
                    >
                      {label}
                    </Button>
                  ))}
                </div>
              </div>
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
                      "尚未关联路由，编辑账号后选择本机日志中的供应商。",
                      "No routes linked. Edit this account to select providers from local logs.",
                    )}
                  </p>
                )}
              </div>
              {usage.isError ? (
                <p role="alert" className="text-caption text-danger-text">
                  {l("本机用量读取失败。", "Could not load local usage.")}
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
                  {l("正在读取本机用量…", "Loading local usage…")}
                </p>
              ) : (
                usage.data?.linked && (
                  <>
                    <div className="grid grid-cols-3 gap-3 border-y border-border py-3">
                      {[
                        [
                          l("请求数", "Requests"),
                          usage.data.summary.totalRequests.toLocaleString(
                            locale,
                          ),
                        ],
                        [
                          l("Tokens", "Tokens"),
                          formatTokensCompact(
                            usage.data.summary.realTotalTokens,
                            locale,
                          ),
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
                                  {v.summary.totalRequests.toLocaleString(
                                    locale,
                                  )}
                                </td>
                                <td className="text-end tabular-nums">
                                  {formatTokensCompact(
                                    v.summary.realTotalTokens,
                                    locale,
                                  )}
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
                  "仅统计关联路由的本机日志，不能区分其不同 Key；折算金额不代表实际扣款或套餐剩余。",
                  "Includes this device's linked routes; individual keys cannot be distinguished. Priced usage is not an invoice or remaining allowance.",
                )}{" "}
                {usage.data &&
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
              queryKey: ["provider-account-refresh", view.account.id],
            });
          }}
        />
      )}
    </>
  );
}
