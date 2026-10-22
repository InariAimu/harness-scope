import { OptionSelect } from "@/components/ui/option-select";
import { useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Trash2 } from "lucide-react";
import { Input } from "@/components/ui/input";
import { DatePicker } from "@/components/ui/date-picker";
import { Checkbox } from "@/components/ui/checkbox";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetBody,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { appShortName } from "@/components/usage/RequestLogTable";
import { providerAccountsApi as api } from "@/lib/api/providerAccounts";
import {
  ACCOUNT_PROVIDER_NAMES,
  accountError,
  routeKey,
  accountProviderName,
  isCustomProvider,
} from "@/lib/providerAccounts";
import { useUnsavedChangesTracker } from "@/lib/unsavedChanges";
import { toast } from "@/lib/toast";
import { CODEX_PLANS } from "@/lib/codexSubscription";
import { codexAllowancesApi } from "@/lib/api/codexAllowances";
import { CustomQueryEditor, defaultCustomQuery } from "@/components/providers/CustomQueryEditor";
import type {
  AccountProvider,
  AccountSave,
  ProviderAccountView,
  CodexPlan,
} from "@/types/providerAccounts";

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="flex flex-col gap-1.5 text-caption text-fg-2">
      {label}
      {children}
    </label>
  );
}
export function ProviderAccountEditor({
  initial,
  provider,
  accounts,
  timezone,
  onClose,
  onSaved,
}: {
  initial?: ProviderAccountView;
  provider: AccountProvider;
  accounts: ProviderAccountView[];
  timezone: string;
  onClose: () => void;
  onSaved: (view: ProviderAccountView) => void;
}) {
  const { i18n } = useTranslation();
  const zh = i18n.language.startsWith("zh");
  const l = (a: string, b: string) => (zh ? a : b);
  const client = useQueryClient();
  const tracker = useUnsavedChangesTracker();
  // Capture runs before controlled inputs' onChange; avoid re-rendering their old values here.
  const dirty = useRef(false);
  const [busy, setBusy] = useState(false);
  const [testing, setTesting] = useState(false);
  const [routeHarness, setRouteHarness] = useState("codex");
  const [routeId, setRouteId] = useState("");
  const [key, setKey] = useState("");
  const [clearKey, setClearKey] = useState(false);
  const [form, setForm] = useState<AccountSave>(() =>
    initial
      ? { ...initial.account }
      : {
          provider: isCustomProvider(provider) ? "custom" : provider,
          name: accountProviderName(provider, [], zh),
          showInSidebar: true,
          links: [],
          subscription: {
            planName: "",
            monthlyFee: null,
            currency: "USD",
            periodStart: null,
            renewalDate: null,
          },
          refresh: { foregroundSeconds: 60, backgroundSeconds: 300 },
          ...(isCustomProvider(provider) ? { custom: defaultCustomQuery() } : {}),
          ...(provider === "openai-codex"
            ? {
                codex: {
                  plan: "plus" as const,
                  binaryPath: null,
                  homePath: null,
                },
              }
            : {}),
        },
  );
  const routes = useQuery({
    queryKey: ["usage", "provider-account-routes"],
    queryFn: api.routes,
    staleTime: 30_000,
  });
  const sharedSubscriptions = useQuery({
    queryKey: ["shared-subscriptions"],
    queryFn: api.sharedSubscriptions,
    staleTime: 0,
    refetchInterval: 5_000,
  });
  const selected = new Set(form.links.map(routeKey));
  const codexRules = useQuery({
    queryKey: ["codex-allowances"],
    queryFn: codexAllowancesApi.info,
    enabled: form.provider === "openai-codex",
    staleTime: 30_000,
  });
  const codexPlans =
    codexRules.data?.catalog.plans ??
    Object.entries(CODEX_PLANS).map(([id, p]) => ({
      id,
      name: p.name,
      monthlyFeeUsd: String(p.monthlyUsd),
      theoreticalMultiplier: p.multiplier,
    }));
  const available = [...(routes.data ?? [])].filter(
    (v) =>
      form.provider !== "openai-codex" ||
      (v.harness === "codex" && v.provider === "openai"),
  );
  for (const link of form.links)
    if (!available.some((v) => routeKey(v) === routeKey(link)))
      available.push({ ...link, records: 0 });
  const updateSub = (field: string, value: string | null) => {
    tracker.onChangeCapture();
    dirty.current = true;
    setForm((v) => ({
      ...v,
      subscription: { ...v.subscription, [field]: value },
    }));
  };
  const save = async () => {
    setBusy(true);
    try {
      const view = await api.save({
        ...form,
        apiKey: key.trim() || undefined,
        clearApiKey: form.provider === "openai-codex" ? undefined : clearKey,
      });
      setKey("");
      onSaved(view);
      toast.success(l("供应商账号已保存", "Provider account saved"));
    } catch (e) {
      toast.error(accountError(String(e), zh));
    } finally {
      setBusy(false);
    }
  };
  const remove = async () => {
    if (
      !initial ||
      !window.confirm(
        l(
          initial.account.provider === "openai-codex"
            ? "删除此账号？本机 Codex 登录和已有用量日志会保留。"
            : "删除此账号及本机保存的 Key？已有用量日志会保留。",
          initial.account.provider === "openai-codex"
            ? "Delete this account? Codex login and usage logs will be kept."
            : "Delete this account and its saved key? Usage logs will be kept.",
        ),
      )
    )
      return;
    setBusy(true);
    try {
      await api.remove(initial.account.id);
      await client.invalidateQueries({ queryKey: ["provider-accounts"] });
      onClose();
    } catch (e) {
      toast.error(accountError(String(e), zh));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Sheet
      open
      onOpenChange={(open) => {
        if (
          !open &&
          !busy && !testing &&
          (!dirty.current ||
            window.confirm(l("放弃未保存的修改？", "Discard unsaved changes?")))
        )
          onClose();
      }}
    >
      <SheetContent width={480} closeLabel={l("关闭", "Close")}>
        <SheetHeader>
          <SheetTitle>
            {initial
              ? l("编辑供应商账号", "Edit provider account")
              : l("添加供应商账号", "Add provider account")}
          </SheetTitle>
          <SheetDescription>
            {l(
              "凭据仅在本机保存，不参与用量同步。",
              "Credentials stay on this device and are excluded from usage sync.",
            )}
          </SheetDescription>
        </SheetHeader>
        <SheetBody>
          <form
            id="provider-account-form"
            className="space-y-4"
            {...tracker}
            onChangeCapture={() => {
              tracker.onChangeCapture();
              dirty.current = true;
            }}
            onInputCapture={() => {
              tracker.onInputCapture();
              dirty.current = true;
            }}
            onSubmit={(e) => {
              e.preventDefault();
              void save();
            }}
          >
            <fieldset disabled={busy || testing} className="min-w-0 space-y-4">
              <Field label={l("供应商", "Provider")}>
                <OptionSelect
                  aria-label={l("供应商类型", "Provider type")}
                  className="scope-select w-full"
                  value={isCustomProvider(form.provider) ? "custom" : form.provider}
                  disabled={!!initial}
                  onValueChange={(value) => {
                    tracker.onChangeCapture();
                    dirty.current = true;
                    const provider = value as AccountProvider;
                    setForm((v) => ({
                      ...v,
                      provider,
                      name: accountProviderName(provider, [], zh),
                      custom: provider === "custom" ? defaultCustomQuery() : undefined,
                      codex:
                        provider === "openai-codex"
                          ? { plan: "plus", binaryPath: null, homePath: null }
                          : undefined,
                      links:
                        provider === "openai-codex"
                          ? v.links.filter(
                              (link) =>
                                link.harness === "codex" &&
                                link.provider === "openai",
                            )
                          : v.links,
                    }));
                    setKey("");
                    setClearKey(false);
                  }}
                >
                  {Object.entries(ACCOUNT_PROVIDER_NAMES).map(([id, name]) => (
                    <option key={id} value={id}>
                      {name}
                    </option>
                  ))}
                  <option value="custom">{l("自定义供应商", "Custom provider")}</option>
                </OptionSelect>
              </Field>
              <Field label={isCustomProvider(form.provider) ? l("供应商名称", "Provider name") : l("账号名称", "Account name")}>
                <Input
                  aria-label={l("账号名称", "Account name")}
                  value={form.name}
                  maxLength={80}
                  required
                  onChange={(e) =>
                    setForm((v) => ({ ...v, name: e.target.value }))
                  }
                />
              </Field>
              <label className="flex items-center gap-2 text-caption text-fg-2">
                <Checkbox
                  checked={form.showInSidebar !== false}
                  onCheckedChange={(showInSidebar) =>
                    setForm((v) => ({ ...v, showInSidebar }))
                  }
                />
                {l("在左侧供应商列表中显示", "Show in sidebar providers")}
              </label>
              <Field label={l("跨终端订阅历史", "Subscription history across devices")}>
                <OptionSelect
                  aria-label={l("跨终端订阅历史", "Subscription history across devices")}
                  value={form.sharedSubscriptionId ?? ""}
                  disabled={!!initial?.account.sharedSubscriptionId}
                  onValueChange={(value) => {
                    tracker.onChangeCapture();
                    dirty.current = true;
                    setForm((v) => ({ ...v, sharedSubscriptionId: value || undefined }));
                  }}
                  className="w-full"
                >
                  <option value="">{l("仅保存在本机", "Keep on this device")}</option>
                  <option value="new">{l("创建共享订阅并同步已有历史", "Create shared subscription and sync existing history")}</option>
                  {(sharedSubscriptions.data ?? []).filter((s) =>
                    s.provider === (isCustomProvider(form.provider) ? "custom" : form.provider) &&
                    !accounts.some((a) => a.account.id !== initial?.account.id && a.account.sharedSubscriptionId === s.id)
                  ).map((s) => <option key={s.id} value={s.id}>{s.name} · {s.id.slice(0, 8)}</option>)}
                  {initial?.account.sharedSubscriptionId && !sharedSubscriptions.data?.some((s) => s.id === initial.account.sharedSubscriptionId) &&
                    <option value={initial.account.sharedSubscriptionId}>{initial.account.sharedSubscriptionId}</option>}
                </OptionSelect>
              </Field>
              <p className="text-caption text-fg-3">
                {l("第一台终端创建共享订阅；其他终端同步后选择同一项。关联会补传本账号已有历史，请确认它们属于同一订阅。各终端可同时使用；保持同步以合并用量，额度回退的异常采样会被筛除。凭据和查询配置各端单独设置。", "Create a shared subscription on the first device; sync and select it on the next. Linking uploads this account’s existing history, so verify it belongs to that subscription. Devices can run concurrently; sync to merge usage. Regressing quota samples are filtered out. Credentials and query settings stay local.")}
                {initial?.account.sharedSubscriptionId && l(" 已关联的账号不能改绑；使用另一份订阅时请添加新账号。", " This binding is fixed; add a new account for another subscription.")}
              </p>
              {form.provider === "openai-codex" ? (
                <section className="rounded-control border border-border p-3 space-y-3">
                  <p className="text-caption text-fg-3">
                    {l(
                      "使用本机 Codex 的 ChatGPT 登录读取订阅额度，无需填写 API Key。",
                      "Reads subscription limits from a local Codex ChatGPT login; no API key is needed.",
                    )}
                  </p>
                  <Field label={l("参考套餐档位", "Reference plan")}>
                    <OptionSelect
                      aria-label={l("参考套餐档位", "Reference plan")}
                      className="scope-select w-full"
                      value={form.codex?.plan ?? "plus"}
                      onValueChange={(value) => {
                        tracker.onChangeCapture();
                        dirty.current = true;
                        setForm((v) => ({
                          ...v,
                          codex: {
                            binaryPath: null,
                            homePath: null,
                            ...v.codex,
                            plan: value as CodexPlan,
                          },
                        }));
                      }}
                    >
                      {codexPlans.map((p) => (
                        <option key={p.id} value={p.id}>
                          {p.name} · ${p.monthlyFeeUsd}/mo ·{" "}
                          {p.theoreticalMultiplier}×
                        </option>
                      ))}
                    </OptionSelect>
                  </Field>
                  <p className="text-caption text-fg-3">
                    {l(
                      "月费只表示订阅价格；参考额度来自本地分模型配置。不会覆盖下方手动月费和续费日期。",
                      "Monthly fees are subscription prices; reference allowances come from local model-specific rules. Manual billing details below are kept.",
                    )}
                  </p>
                  <Field
                    label={l(
                      "Codex 程序路径（可选）",
                      "Codex executable (optional)",
                    )}
                  >
                    <Input
                      aria-label={l("Codex 程序路径", "Codex executable")}
                      value={form.codex?.binaryPath ?? ""}
                      placeholder={l(
                        "留空自动检测桌面版或 PATH 中的 Codex",
                        "Auto-detect desktop or PATH Codex",
                      )}
                      onChange={(e) =>
                        setForm((v) => ({
                          ...v,
                          codex: {
                            plan: "plus",
                            homePath: null,
                            ...v.codex,
                            binaryPath: e.target.value || null,
                          },
                        }))
                      }
                    />
                  </Field>
                  <Field
                    label={l("Codex 数据目录（可选）", "Codex home (optional)")}
                  >
                    <Input
                      aria-label={l("Codex 数据目录", "Codex home")}
                      value={form.codex?.homePath ?? ""}
                      placeholder={l(
                        "留空使用 CODEX_HOME 或 ~/.codex",
                        "CODEX_HOME or ~/.codex by default",
                      )}
                      onChange={(e) =>
                        setForm((v) => ({
                          ...v,
                          codex: {
                            plan: "plus",
                            binaryPath: null,
                            ...v.codex,
                            homePath: e.target.value || null,
                          },
                        }))
                      }
                    />
                  </Field>
                </section>
              ) : (
                <Field label="API Key">
                  <Input
                    aria-label="API Key"
                    type="password"
                    autoComplete="new-password"
                    value={key}
                    disabled={clearKey}
                    maxLength={4096}
                    placeholder={
                      initial?.keyConfigured
                        ? l(
                            "已保存；留空保留原 Key",
                            "Saved; leave blank to keep the current key",
                          )
                        : l("可稍后配置", "Can be configured later")
                    }
                    onChange={(e) => setKey(e.target.value)}
                  />
                </Field>
              )}
              {form.provider !== "openai-codex" && initial?.keyConfigured && (
                <label className="flex items-center gap-2 text-caption text-fg-2">
                  <Checkbox
                    checked={clearKey}
                    onCheckedChange={(v) => {
                      setClearKey(v);
                      if (v) setKey("");
                    }}
                  />
                  {l("清除已保存的 Key", "Remove saved key")}
                </label>
              )}
              {form.custom && <CustomQueryEditor value={form.custom} id={initial?.account.id} apiKey={key} clearApiKey={clearKey} zh={zh} timezone={timezone} onTesting={setTesting}
                onChange={custom => { tracker.onChangeCapture(); dirty.current = true; setForm(v => ({ ...v, custom })); }} />}
              <section className="border-t border-border pt-4 space-y-3">
                <h3 className="font-semibold text-fg-1">
                  {l("订阅信息", "Subscription")}{" "}
                  <span className="font-normal text-caption text-fg-3">
                    {l("手动填写", "Manual")}
                  </span>
                </h3>
                <Field label={l("套餐名称", "Plan name")}>
                  <Input
                    aria-label={l("套餐名称", "Plan name")}
                    maxLength={80}
                    value={form.subscription.planName}
                    placeholder={l("例如 Go Plus", "e.g. Go Plus")}
                    onChange={(e) => updateSub("planName", e.target.value)}
                  />
                </Field>
                <div className="grid grid-cols-[1fr_110px] gap-3">
                  <Field label={l("每月费用", "Monthly fee")}>
                    <Input
                      aria-label={l("每月费用", "Monthly fee")}
                      type="number"
                      min="0"
                      max="1000000"
                      step="any"
                      value={form.subscription.monthlyFee ?? ""}
                      onChange={(e) =>
                        updateSub("monthlyFee", e.target.value || null)
                      }
                    />
                  </Field>
                  <Field label={l("币种", "Currency")}>
                    <OptionSelect
                      aria-label={l("订阅币种", "Subscription currency")}
                      className="scope-select w-full"
                      value={form.subscription.currency}
                      onValueChange={(value) => updateSub("currency", value)}
                    >
                      {["USD", "CNY", "EUR", "JPY", "TWD", "TRY"].map((v) => (
                        <option key={v}>{v}</option>
                      ))}
                    </OptionSelect>
                  </Field>
                </div>
                <div className="grid grid-cols-2 gap-3">
                  <Field label={l("本期开始", "Period start")}>
                    <DatePicker
                      aria-label={l("本期开始", "Period start")}
                      value={form.subscription.periodStart ?? ""}
                      onValueChange={(value) =>
                        updateSub("periodStart", value || null)
                      }
                    />
                  </Field>
                  <Field label={l("下次续费", "Next renewal")}>
                    <DatePicker
                      aria-label={l("下次续费", "Next renewal")}
                      value={form.subscription.renewalDate ?? ""}
                      onValueChange={(value) =>
                        updateSub("renewalDate", value || null)
                      }
                    />
                  </Field>
                </div>
              </section>
              <section className="border-t border-border pt-4 space-y-3">
                <h3 className="font-semibold text-fg-1">
                  {l("余额／额度自动刷新", "Automatic balance / quota refresh")}
                </h3>
                <div className="grid grid-cols-2 gap-3">
                  {(
                    [
                      [
                        "foregroundSeconds",
                        l("前台间隔（秒）", "Foreground interval (seconds)"),
                      ],
                      [
                        "backgroundSeconds",
                        l("后台间隔（秒）", "Background interval (seconds)"),
                      ],
                    ] as const
                  ).map(([field, label]) => (
                    <Field key={field} label={label}>
                      <Input
                        aria-label={label}
                        type="number"
                        required
                        min={0}
                        max={86400}
                        step={1}
                        value={form.refresh[field]}
                        onChange={(e) =>
                          setForm((v) => ({
                            ...v,
                            refresh: {
                              ...v.refresh,
                              [field]: Number(e.target.value),
                            },
                          }))
                        }
                      />
                    </Field>
                  ))}
                </div>
                <p className="text-caption text-fg-3">
                  {l(
                    "0 关闭自动刷新；其余为 15–86400 秒。前台指当前供应商页有焦点；切换页面、窗口失焦或最小化后用后台间隔。关闭应用后停止。手动刷新始终可用。",
                    "Use 0 to disable, or 15–86400 seconds. Foreground means the selected provider page has focus. Other pages, unfocused or minimized windows use the background interval. Refresh stops when the app closes. Manual refresh is always available.",
                  )}
                </p>
              </section>
              <section className="border-t border-border pt-4 space-y-2">
                <h3 className="font-semibold text-fg-1">
                  {isCustomProvider(form.provider) ? l("Provider ID 归属", "Provider ID links") : l("关联跨设备用量", "Link usage across devices")}
                </h3>
                {isCustomProvider(form.provider) && <div className="space-y-2">
                  <p className="text-caption text-fg-3">{l("按工具和原始 provider ID 精确关联，在会话与请求详情中显示此供应商名称。", "Match the tool and original provider ID exactly to display this provider in sessions and requests.")}</p>
                  <div className="flex gap-2">
                    <OptionSelect aria-label={l("关联工具", "Linked tool")} className="scope-select w-28 shrink-0" value={routeHarness} onValueChange={setRouteHarness}>
                      {["codex", "claude", "opencode", "dsh", "gemini", "copliot"].map(id => <option key={id} value={id}>{appShortName(id)}</option>)}
                    </OptionSelect>
                    <Input aria-label="Provider ID" placeholder="provider-id" maxLength={128} value={routeId} onChange={e => setRouteId(e.target.value)} />
                    <Button type="button" variant="neutral" disabled={!routeId.trim()} onClick={() => {
                      const link = { harness: routeHarness, provider: routeId.trim() };
                      const owner = accounts.find(v => v.account.id !== initial?.account.id && v.account.links.some(l => routeKey(l) === routeKey(link)));
                      if (owner) { toast.error(l(`此 ID 已关联 ${owner.account.name}`, `This ID is linked to ${owner.account.name}`)); return; }
                      tracker.onChangeCapture(); dirty.current = true;
                      setForm(v => ({ ...v, links: v.links.some(l => routeKey(l) === routeKey(link)) ? v.links : [...v.links, link] }));
                      setRouteId("");
                    }}>{l("添加", "Add")}</Button>
                  </div>
                </div>}
                <p className="text-caption text-fg-3">
                  {l(
                    "选择所有已同步设备日志中的路由，按共用同一组订阅汇总。一条路由只能关联一个账号，日志无法区分其不同账号或 Key。",
                    "Choose routes from all synced devices, assuming they share a subscription. A route can belong to one account; logs cannot distinguish accounts or keys on the same route.",
                  )}
                </p>
                {routes.isError && (
                  <p role="alert" className="text-caption text-danger-text">
                    {l(
                      "路由读取失败，请重试。",
                      "Could not load routes. Please retry.",
                    )}
                  </p>
                )}
                {routes.isLoading && (
                  <p className="text-caption text-fg-3">
                    {l("正在读取路由…", "Loading routes…")}
                  </p>
                )}
                {!routes.isLoading && !available.length && (
                  <p className="text-caption text-fg-3">
                    {l(
                      "暂无带供应商信息的用量日志，请先采集或同步。",
                      "No logs with provider information yet. Collect or sync usage first.",
                    )}
                  </p>
                )}
                <div className="max-h-52 overflow-y-auto rounded-control border border-border divide-y divide-border">
                  {available.map((route) => {
                    const key = routeKey(route);
                    const owner = accounts.find(
                      (v) =>
                        v.account.id !== form.id &&
                        v.account.links.some((v) => routeKey(v) === key),
                    );
                    return (
                      <label
                        key={key}
                        className="flex items-center gap-2.5 px-3 py-2 text-caption"
                      >
                        <Checkbox
                          aria-label={`${appShortName(route.harness)} / ${route.provider}`}
                          checked={selected.has(key)}
                          disabled={!!owner}
                          onCheckedChange={(checked) =>
                            setForm((v) => ({
                              ...v,
                              links: checked
                                ? [
                                    ...v.links,
                                    {
                                      harness: route.harness,
                                      provider: route.provider,
                                    },
                                  ]
                                : v.links.filter((v) => routeKey(v) !== key),
                            }))
                          }
                        />
                        <span
                          className="min-w-0 flex-1 truncate"
                          title={route.provider}
                        >
                          {appShortName(route.harness)} / {route.provider}
                        </span>
                        <span className="shrink-0 text-fg-3">
                          {owner
                            ? owner.account.name
                            : route.records.toLocaleString()}
                        </span>
                      </label>
                    );
                  })}
                </div>
              </section>
            </fieldset>
          </form>
        </SheetBody>
        <SheetFooter>
          {initial && (
            <Button
              variant="ghost"
              className="me-auto text-danger-text"
              disabled={busy || testing}
              onClick={() => void remove()}
            >
              <Trash2 size={14} />
              {l("删除", "Delete")}
            </Button>
          )}
          <Button
            variant="neutral"
            disabled={busy || testing}
            onClick={() => {
              if (
                !dirty.current ||
                window.confirm(
                  l("放弃未保存的修改？", "Discard unsaved changes?"),
                )
              )
                onClose();
            }}
          >
            {l("取消", "Cancel")}
          </Button>
          <Button type="submit" form="provider-account-form" disabled={busy || testing}>
            {busy ? l("保存中…", "Saving…") : l("保存", "Save")}
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}
