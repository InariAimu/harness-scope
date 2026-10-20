import { useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Trash2 } from "lucide-react";
import { Input } from "@/components/ui/input";
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
} from "@/lib/providerAccounts";
import { useUnsavedChangesTracker } from "@/lib/unsavedChanges";
import { toast } from "@/lib/toast";
import type {
  AccountProvider,
  AccountSave,
  ProviderAccountView,
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
  onClose,
  onSaved,
}: {
  initial?: ProviderAccountView;
  provider: AccountProvider;
  accounts: ProviderAccountView[];
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
  const [key, setKey] = useState("");
  const [clearKey, setClearKey] = useState(false);
  const [form, setForm] = useState<AccountSave>(() =>
    initial
      ? { ...initial.account }
      : {
          provider,
          name: ACCOUNT_PROVIDER_NAMES[provider],
          links: [],
          subscription: {
            planName: "",
            monthlyFee: null,
            currency: "USD",
            periodStart: null,
            renewalDate: null,
          },
          refresh: { foregroundSeconds: 60, backgroundSeconds: 300 },
        },
  );
  const routes = useQuery({
    queryKey: ["provider-account-routes"],
    queryFn: api.routes,
    staleTime: 30_000,
  });
  const selected = new Set(form.links.map(routeKey));
  const available = [...(routes.data ?? [])];
  for (const link of form.links)
    if (!available.some((v) => routeKey(v) === routeKey(link)))
      available.push({ ...link, records: 0 });
  const updateSub = (field: string, value: string | null) =>
    setForm((v) => ({
      ...v,
      subscription: { ...v.subscription, [field]: value },
    }));
  const save = async () => {
    setBusy(true);
    try {
      const view = await api.save({
        ...form,
        apiKey: key.trim() || undefined,
        clearApiKey: clearKey,
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
          "删除此账号及本机保存的 Key？已有用量日志会保留。",
          "Delete this account and its saved key? Usage logs will be kept.",
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
          !busy &&
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
            <fieldset disabled={busy} className="min-w-0 space-y-4">
              <Field label={l("供应商", "Provider")}>
                <select
                  aria-label={l("供应商类型", "Provider type")}
                  className="scope-select w-full"
                  value={form.provider}
                  disabled={!!initial}
                  onChange={(e) => {
                    const provider = e.target.value as AccountProvider;
                    setForm((v) => ({
                      ...v,
                      provider,
                      name: ACCOUNT_PROVIDER_NAMES[provider],
                    }));
                  }}
                >
                  {Object.entries(ACCOUNT_PROVIDER_NAMES).map(([id, name]) => (
                    <option key={id} value={id}>
                      {name}
                    </option>
                  ))}
                </select>
              </Field>
              <Field label={l("账号名称", "Account name")}>
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
              {initial?.keyConfigured && (
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
                    <select
                      aria-label={l("订阅币种", "Subscription currency")}
                      className="scope-select w-full"
                      value={form.subscription.currency}
                      onChange={(e) => updateSub("currency", e.target.value)}
                    >
                      {["USD", "CNY", "EUR", "JPY", "TWD", "TRY"].map((v) => (
                        <option key={v}>{v}</option>
                      ))}
                    </select>
                  </Field>
                </div>
                <div className="grid grid-cols-2 gap-3">
                  <Field label={l("本期开始", "Period start")}>
                    <Input
                      aria-label={l("本期开始", "Period start")}
                      type="date"
                      value={form.subscription.periodStart ?? ""}
                      onChange={(e) =>
                        updateSub("periodStart", e.target.value || null)
                      }
                    />
                  </Field>
                  <Field label={l("下次续费", "Next renewal")}>
                    <Input
                      aria-label={l("下次续费", "Next renewal")}
                      type="date"
                      value={form.subscription.renewalDate ?? ""}
                      onChange={(e) =>
                        updateSub("renewalDate", e.target.value || null)
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
                  {l("关联本机用量", "Link local usage")}
                </h3>
                <p className="text-caption text-fg-3">
                  {l(
                    "选择日志中的路由。一条路由只能关联一个账号，无法从历史日志区分其不同 Key。",
                    "Select routes from local logs. A route can belong to one account; historical logs do not identify individual keys.",
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
                      "暂无带供应商信息的本机日志。",
                      "No local logs with provider information yet.",
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
              disabled={busy}
              onClick={() => void remove()}
            >
              <Trash2 size={14} />
              {l("删除", "Delete")}
            </Button>
          )}
          <Button
            variant="neutral"
            disabled={busy}
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
          <Button type="submit" form="provider-account-form" disabled={busy}>
            {busy ? l("保存中…", "Saving…") : l("保存", "Save")}
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}
