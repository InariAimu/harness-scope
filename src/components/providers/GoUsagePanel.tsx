import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Copy, FileInput, SlidersHorizontal } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { goCatalogApi as api } from "@/lib/api/goCatalog";
import { goError, goPercent, goQueryPrompt } from "@/lib/goCatalog";
import { copyText } from "@/lib/clipboard";
import { toast } from "@/lib/toast";
import { nativeMoney } from "@/lib/providerAccounts";
import type { ProviderAccountView } from "@/types/providerAccounts";
import type {
  GoCatalog,
  GoCatalogInfo,
  GoPlan,
  GoSimulation,
  GoTokens,
  GoWindowId,
} from "@/types/goCatalog";
import { GoRulesEditor } from "./GoRulesEditor";
import { GoForecastPanel } from "./GoForecastPanel";
const emptyTokens: GoTokens = {
  input: 0,
  output: 0,
  cacheRead: 0,
  cacheWrite: 0,
  cacheWrite1h: 0,
  reasoning: 0,
};
function initialPlan(view: ProviderAccountView): GoPlan {
  try {
    const saved = localStorage.getItem(
      `harness-scope.go-plan:${view.account.id}`,
    );
    if (saved === "go" || saved === "goPlus") return saved;
  } catch {
    /* use the visible manual plan name */
  }
  return view.account.subscription.planName
    .toLowerCase()
    .replace(/[\s_-]/g, "") === "goplus"
    ? "goPlus"
    : "go";
}
export function GoUsagePanel({
  view,
  zh,
  locale,
}: {
  view: ProviderAccountView;
  zh: boolean;
  locale: string;
}) {
  const l = (a: string, b: string) => (zh ? a : b);
  const [plan, setPlan] = useState<GoPlan>(() => initialPlan(view));
  const [tab, setTab] = useState("forecast");
  const [windowId, setWindowId] = useState<GoWindowId>("monthly");
  const [editor, setEditor] = useState<{
    info: GoCatalogInfo;
    draft: GoCatalog;
  } | null>(null);
  const rules = useQuery({
    queryKey: ["go-catalog"],
    queryFn: api.info,
    staleTime: 15_000,
    refetchInterval: 30_000,
  });
  const usage = useQuery({
    queryKey: [
      "usage",
      "go-usage",
      view.account.id,
      plan,
      rules.data?.revision,
      view.account.links,
      view.account.subscription.periodStart,
      view.snapshot?.fetchedAt,
    ],
    queryFn: () => api.usage(view.account.id, plan),
    enabled: !!rules.data,
    staleTime: 30_000,
    refetchInterval: 60_000,
  });
  const catalog = rules.data?.catalog;
  const labels: Record<GoWindowId, string> = {
    rolling: l("五小时", "5 hours"),
    weekly: l("每周", "Weekly"),
    monthly: l("每月", "Monthly"),
  };
  const [modelId, setModelId] = useState("deepseek-flash");
  const [tokens, setTokens] = useState<GoTokens>(emptyTokens);
  const [requests, setRequests] = useState(100);
  const [period, setPeriod] = useState("current");
  const [result, setResult] = useState<GoSimulation | null>(null);
  const [calculationError, setCalculationError] = useState("");
  const [calculating, setCalculating] = useState(false);
  const model =
    catalog?.models.find((v) => v.id === modelId) ?? catalog?.models[0];
  useEffect(() => {
    if (catalog && !catalog.models.some((v) => v.id === modelId)) {
      setModelId(catalog.models[0].id);
      setTokens(catalog.models[0].sampleTokens ?? emptyTokens);
    }
  }, [rules.data?.revision, modelId]);
  const recent = usage.data?.recentHour.find((v) => v.model === model?.id);
  const showWindow = usage.data?.windows.find((v) => v.id === windowId);
  const money = (value: string | null) => nativeMoney(value, "USD", locale);
  const changePlan = (v: GoPlan) => {
    setPlan(v);
    setResult(null);
    try {
      localStorage.setItem(`harness-scope.go-plan:${view.account.id}`, v);
    } catch {
      /* preference remains for this page */
    }
  };
  const calculate = async () => {
    if (!model) return;
    setCalculating(true);
    setCalculationError("");
    setResult(null);
    try {
      setResult(
        await api.simulate({ model: model.id, plan, requests, tokens, period }),
      );
    } catch (e) {
      setCalculationError(goError(e, zh));
    } finally {
      setCalculating(false);
    }
  };
  const copy = async () => {
    if (!rules.data) return;
    try {
      await copyText(goQueryPrompt(rules.data));
      toast.success(
        l(
          "查询提示词已复制，不包含凭据",
          "Query prompt copied; no credentials included",
        ),
      );
    } catch (e) {
      toast.error(String(e));
    }
  };
  const pendingFile =
    rules.data?.file.catalog &&
    JSON.stringify(rules.data.file.catalog) !==
      JSON.stringify(rules.data.catalog);
  return (
    <section className="scope-card !p-4 space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="font-semibold">
          {l("Go 额度估算", "Go allowance estimates")}
        </h2>
        <div className="flex flex-wrap gap-2">
          <Button
            variant="neutral"
            size="compact"
            disabled={!rules.data}
            onClick={() => void copy()}
          >
            <Copy size={13} />
            {l("复制查询提示词", "Copy query prompt")}
          </Button>
          <Button
            variant="neutral"
            size="compact"
            disabled={!rules.data}
            onClick={() =>
              rules.data &&
              setEditor({ info: rules.data, draft: rules.data.catalog })
            }
          >
            <SlidersHorizontal size={13} />
            {l("编辑规则", "Edit rules")}
          </Button>
        </div>
      </div>
      {rules.isError && (
        <p role="alert" className="text-caption text-danger-text">
          {goError(rules.error, zh)}
        </p>
      )}
      <div className="flex flex-wrap items-center gap-2">
        <label htmlFor="go-estimate-plan" className="text-caption text-fg-2">
          {l("估算套餐", "Estimation plan")}
        </label>
        <select
          id="go-estimate-plan"
          className="scope-select"
          value={plan}
          onChange={(e) => changePlan(e.target.value as GoPlan)}
        >
          <option value="go">Go</option>
          <option value="goPlus">Go Plus</option>
        </select>
        <span className="text-caption text-fg-3">
          {l(
            "仅用于估算，不修改手填订阅",
            "For estimates; does not change the manual subscription",
          )}
        </span>
      </div>
      <div className="flex flex-wrap gap-1 border-b border-border pb-2">
        {[
          [
            "forecast",
            l("额度续航与每日预算", "Time remaining and daily budget"),
          ],
          ["usage", l("本机分模型用量", "Local model usage")],
          ["simulate", l("用量模拟器", "Usage simulator")],
          ["rules", l("模型价表与额度", "Model rates and limits")],
        ].map(([id, label]) => (
          <Button
            key={id}
            size="compact"
            variant={tab === id ? "neutral" : "quiet"}
            aria-pressed={tab === id}
            onClick={() => setTab(id)}
          >
            {label}
          </Button>
        ))}
      </div>
      {tab === "forecast" && rules.data && (
        <GoForecastPanel
          view={view}
          info={rules.data}
          plan={plan}
          zh={zh}
          locale={locale}
        />
      )}
      {tab === "usage" && (
        <div className="space-y-3">
          <p className="text-caption text-fg-3">
            {l(
              "按当前本地表逐请求计价，再除以该模型的套餐额度；不同模型的额度比例相加。此处不会改写历史费用。",
              "Each request is priced from the current local table and divided by its model allowance. Model percentages are added. Historical costs remain unchanged.",
            )}
          </p>
          {usage.isError ? (
            <p role="alert" className="text-caption text-danger-text">
              {goError(usage.error, zh)}
            </p>
          ) : usage.isLoading ? (
            <p className="text-caption text-fg-3">
              {l("正在读取本机用量…", "Loading local usage…")}
            </p>
          ) : (
            <>
              <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                {usage.data?.windows.map((window) => (
                  <button
                    key={window.id}
                    className="rounded-panel border border-border p-3 text-start space-y-1 hover:bg-subtle"
                    aria-pressed={windowId === window.id}
                    onClick={() => setWindowId(window.id)}
                  >
                    <span className="text-caption text-fg-3">
                      {labels[window.id]} ·{" "}
                      {l("本机估算占用", "Estimated local usage")}
                    </span>
                    <strong className="block text-xl tabular-nums">
                      {window.total.unmatchedRequests ? "≥ " : "≈ "}
                      {goPercent(window.total.percent)}
                    </strong>
                    <span className="block text-caption text-fg-3">
                      {window.total.requests.toLocaleString(locale)}{" "}
                      {l("请求", "requests")}
                      {window.total.unmatchedRequests
                        ? ` · ${window.total.unmatchedRequests} ${l("未计入", "excluded")}`
                        : ""}
                    </span>
                  </button>
                ))}
              </div>
              {showWindow && (
                <>
                  <p className="text-caption text-fg-3">
                    {labels[showWindow.id]}:{" "}
                    {new Date(showWindow.start * 1000).toLocaleString(locale)} –{" "}
                    {new Date(showWindow.end * 1000).toLocaleString(locale)} ·{" "}
                    {showWindow.basis === "provider_window"
                      ? l(
                          "按官方重置时间划定窗口",
                          "Window based on provider reset",
                        )
                      : showWindow.basis === "manual_period"
                        ? l("按手填账期", "Manual billing period")
                        : l(
                            "滚动回看，不代表官方结算窗口",
                            "Lookback; not an official billing window",
                          )}
                  </p>
                  <div className="overflow-x-auto">
                    <table className="w-full text-caption">
                      <thead className="text-fg-3">
                        <tr>
                          {[
                            l("模型原文", "Original model"),
                            l("请求", "Requests"),
                            l("额度折算 USD", "Priced usage USD"),
                            l("窗口占用", "Window usage"),
                          ].map((v) => (
                            <th
                              key={v}
                              className="text-start font-normal py-2 pe-3"
                            >
                              {v}
                            </th>
                          ))}
                        </tr>
                      </thead>
                      <tbody>
                        {showWindow.models.map((v) => (
                          <tr key={v.model} className="border-t border-border">
                            <td
                              className="py-2 pe-3 max-w-56 truncate font-mono"
                              title={`${v.model}${v.pricingModel ? ` → ${v.pricingModel}` : ""}`}
                            >
                              {v.model}
                            </td>
                            <td className="tabular-nums pe-3">
                              {v.usage.requests.toLocaleString(locale)}
                            </td>
                            <td className="tabular-nums pe-3">
                              {v.usage.unmatchedRequests === v.usage.requests
                                ? "—"
                                : money(v.usage.costUsd)}
                            </td>
                            <td className="tabular-nums">
                              {v.usage.unmatchedRequests === v.usage.requests
                                ? l("未能估算", "Unestimated")
                                : `${v.usage.unmatchedRequests ? "≥ " : "≈ "}${goPercent(v.usage.percent)}`}
                            </td>
                          </tr>
                        ))}
                        {!showWindow.models.length && (
                          <tr>
                            <td colSpan={4} className="py-4 text-fg-3">
                              {l(
                                "此窗口没有关联路由的本机请求。",
                                "No linked local requests in this window.",
                              )}
                            </td>
                          </tr>
                        )}
                      </tbody>
                    </table>
                  </div>
                </>
              )}
            </>
          )}
          <p className="text-caption text-fg-3">
            {l(
              "仅包含关联路由的本机日志。未匹配模型、未公布的缓存单价与旧日汇总会明确排除；跨设备使用、促销和窗口边界可能与官方统计不同。上方三窗口卡片仍以官方接口为准。",
              "Includes linked local logs only. Unknown models, unprovided cache rates and legacy daily totals are excluded. Other devices, promotions and window boundaries may differ from provider usage. The provider quota cards above remain authoritative.",
            )}
          </p>
        </div>
      )}
      {tab === "simulate" && (
        <div className="space-y-3">
          <form
            className="space-y-3"
            onSubmit={(e) => {
              e.preventDefault();
              void calculate();
            }}
          >
            <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
              <label className="text-caption text-fg-2 space-y-1">
                {l("模拟模型", "Simulation model")}
                <select
                  aria-label={l("模拟模型", "Simulation model")}
                  className="scope-select w-full"
                  value={model?.id ?? ""}
                  onChange={(e) => {
                    setModelId(e.target.value);
                    setTokens(
                      catalog?.models.find((v) => v.id === e.target.value)
                        ?.sampleTokens ?? emptyTokens,
                    );
                    setResult(null);
                  }}
                >
                  {catalog?.models.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.name}
                    </option>
                  ))}
                </select>
              </label>
              <label className="text-caption text-fg-2 space-y-1">
                {l("请求数", "Simulated requests")}
                <Input
                  aria-label={l("模拟请求数", "Simulated requests")}
                  type="number"
                  required
                  min={0}
                  max={1_000_000_000}
                  step={1}
                  value={requests}
                  onChange={(e) => {
                    setRequests(Number(e.target.value));
                    setResult(null);
                  }}
                />
              </label>
              <label className="text-caption text-fg-2 space-y-1">
                {l("时段", "Period")}
                <select
                  aria-label={l("模拟时段", "Simulation period")}
                  className="scope-select w-full"
                  value={period}
                  onChange={(e) => {
                    setPeriod(e.target.value);
                    setResult(null);
                  }}
                >
                  <option value="current">
                    {l("按当前时段", "Current period")}
                  </option>
                  <option value="off_peak">
                    {l("非高峰情景", "Off-peak scenario")}
                  </option>
                  <option value="peak">{l("高峰情景", "Peak scenario")}</option>
                </select>
              </label>
            </div>
            <div className="grid grid-cols-2 sm:grid-cols-3 gap-3">
              {(
                [
                  ["input", l("每请求未缓存输入", "Uncached input / request")],
                  ["cacheRead", l("每请求缓存读取", "Cache read / request")],
                  [
                    "output",
                    l(
                      "每请求输出（含推理）",
                      "Output incl. reasoning / request",
                    ),
                  ],
                  [
                    "cacheWrite",
                    l("每请求缓存写入总量", "Total cache write / request"),
                  ],
                  ["cacheWrite1h", l("其中 1h 写入", "Of which 1h writes")],
                ] as const
              ).map(([field, label]) => (
                <label key={field} className="text-caption text-fg-2 space-y-1">
                  {label}
                  <Input
                    aria-label={label}
                    type="number"
                    min={0}
                    max={1_000_000_000_000}
                    required
                    step={1}
                    value={tokens[field]}
                    onChange={(e) => {
                      setTokens((v) => ({
                        ...v,
                        [field]: Number(e.target.value),
                      }));
                      setResult(null);
                    }}
                  />
                </label>
              ))}
            </div>
            <div className="flex flex-wrap gap-2">
              <Button
                variant="neutral"
                type="button"
                disabled={!model?.sampleTokens}
                onClick={() => {
                  if (model?.sampleTokens) {
                    setTokens(model.sampleTokens);
                    setResult(null);
                  }
                }}
              >
                {l("填入官方典型请求", "Use official example")}
              </Button>
              <Button
                variant="neutral"
                type="button"
                disabled={!recent}
                onClick={() => {
                  if (recent) {
                    setTokens(recent.averageTokens);
                    setRequests(recent.requests);
                    setResult(null);
                  }
                }}
              >
                {l("使用本机近 1 小时均值", "Use local last-hour average")}
              </Button>
              <Button disabled={calculating || !model} type="submit">
                {calculating
                  ? l("计算中…", "Calculating…")
                  : l("计算额度消耗", "Calculate allowance usage")}
              </Button>
            </div>
            <p className="text-caption text-fg-3">
              {recent
                ? l(
                    `本机近一小时该模型 ${recent.requests} 次请求；填入后模拟相同强度的一小时。`,
                    `This model made ${recent.requests} local requests in the last hour; the preset simulates an hour at that rate.`,
                  )
                : l(
                    "近一小时暂无该模型样本；可填写 Token 数或使用官方典型请求。",
                    "No local samples for this model in the last hour. Enter tokens or use an official example.",
                  )}
            </p>
          </form>
          {calculationError && (
            <p role="alert" className="text-caption text-danger-text">
              {calculationError}
            </p>
          )}
          {result && (
            <div
              className="rounded-panel border border-border p-3 space-y-2"
              role="status"
            >
              <p className="font-medium">
                {l("模拟额度折算", "Simulated priced usage")}:{" "}
                {money(result.costUsd)} ·{" "}
                {result.period === "peak"
                  ? l("高峰", "Peak")
                  : result.period === "off_peak"
                    ? l("非高峰", "Off-peak")
                    : l("固定单价", "Flat rate")}
              </p>
              <div className="grid grid-cols-3 gap-3">
                {(["rolling", "weekly", "monthly"] as const).map((id) => (
                  <div key={id}>
                    <p className="text-caption text-fg-3">{labels[id]}</p>
                    <strong className="tabular-nums">
                      {goPercent(result.percent[id])}
                    </strong>
                  </div>
                ))}
              </div>
            </div>
          )}
          <p className="text-caption text-fg-3">
            {l(
              "模拟不会发送模型请求或修改记录；它计算假设请求的额度占用，不代表新增账单或官方剩余额度。",
              "Simulation sends no model requests and changes no records. It estimates hypothetical allowance usage, not a cash charge or official remaining quota.",
            )}
          </p>
        </div>
      )}
      {tab === "rules" && catalog && (
        <div className="space-y-3">
          <div className="flex flex-wrap items-center justify-between gap-2 text-caption text-fg-3">
            <span>
              {catalog.version} · {l("核查", "Checked")} {catalog.checkedAt} ·{" "}
              {catalog.models.length} {l("模型", "models")}
            </span>
            <a
              className="text-action-text"
              href={catalog.sourceUrl}
              target="_blank"
              rel="noreferrer"
            >
              {l("官方额度表", "Official usage table")}
            </a>
          </div>
          <p className="text-caption text-fg-3">
            UTC · {l("星期", "Weekdays")}{" "}
            {catalog.peakSchedule.weekdays.join(", ")} ·{" "}
            {catalog.peakSchedule.intervals
              .map(([a, b]) =>
                [a, b]
                  .map(
                    (n) =>
                      `${String(Math.floor(n / 60)).padStart(2, "0")}:${String(n % 60).padStart(2, "0")}`,
                  )
                  .join("–"),
              )
              .join(" / ")}
          </p>
          <div className="max-h-96 overflow-auto rounded-control border border-border">
            <table className="w-full text-caption whitespace-nowrap">
              <thead className="bg-subtle text-fg-3">
                <tr>
                  {[
                    l("模型 / 上下文", "Model / context"),
                    l("输入", "Input"),
                    l("输出", "Output"),
                    l("缓存读", "Cache read"),
                    l("缓存写", "Cache write"),
                    `${plan === "goPlus" ? "Go Plus" : "Go"} ${l("月额度", "monthly limit")}`,
                  ].map((v) => (
                    <th key={v} className="p-2 text-start font-normal">
                      {v}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {catalog.models.flatMap((m) =>
                  m.tiers.map((t, i) => (
                    <tr key={`${m.id}:${i}`} className="border-t border-border">
                      <td className="p-2" title={m.aliases.join(", ")}>
                        <div>{m.name}</div>
                        <div className="text-fg-3">
                          {m.id} ·{" "}
                          {t.maxContextTokens
                            ? `≤ ${t.maxContextTokens.toLocaleString(locale)}`
                            : i
                              ? `> ${m.tiers[i - 1].maxContextTokens?.toLocaleString(locale)}`
                              : l("全部上下文", "All contexts")}
                        </div>
                      </td>
                      {(
                        ["input", "output", "cacheRead", "cacheWrite"] as const
                      ).map((k) => (
                        <td key={k} className="p-2 tabular-nums">
                          {t.prices[k] ?? "—"}
                          {t.peakPrices ? ` / ${t.peakPrices[k] ?? "—"}` : ""}
                        </td>
                      ))}
                      <td className="p-2 tabular-nums">
                        {m.free ? "Free / ∞" : money(t.monthlyLimits[plan])}
                      </td>
                    </tr>
                  )),
                )}
              </tbody>
            </table>
          </div>
          <p className="text-caption text-fg-3">
            {l(
              "单价 USD / 百万 Tokens；双值为非高峰 / 高峰；月额度不是订阅月费。— 为未公布，不能当作零价。",
              "Rates are USD per million tokens; pairs show off-peak / peak. Monthly allowance is not the subscription fee. — means unprovided, not zero.",
            )}
          </p>
          <p className="text-caption text-fg-3 break-all">{rules.data?.path}</p>
          {rules.data?.file.error && (
            <p role="alert" className="text-caption text-danger-text">
              {l(
                "本地文件未通过校验，当前规则保持不变。",
                "Local file failed validation; active rules remain unchanged.",
              )}
            </p>
          )}
          {pendingFile && (
            <p className="text-caption text-action-text">
              {l(
                "本地文件有待应用的更改。",
                "The local file has unapplied changes.",
              )}
            </p>
          )}
          <Button
            variant="neutral"
            disabled={!rules.data?.file.catalog}
            onClick={() =>
              rules.data?.file.catalog &&
              setEditor({ info: rules.data, draft: rules.data.file.catalog })
            }
          >
            <FileInput size={13} />
            {l("从文件载入", "Load from file")}
          </Button>
        </div>
      )}
      {editor && (
        <GoRulesEditor
          info={editor.info}
          draft={editor.draft}
          zh={zh}
          onClose={() => setEditor(null)}
        />
      )}
    </section>
  );
}
