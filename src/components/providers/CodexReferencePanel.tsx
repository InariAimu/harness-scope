import { OptionSelect } from "@/components/ui/option-select";
import { useEffect, useId, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { CODEX_PLANS } from "@/lib/codexSubscription";
import { codexAllowancesApi } from "@/lib/api/codexAllowances";
import {
  codexReferenceQuery,
  type CodexReferenceArgs,
} from "@/lib/query/codexReference";
import { CodexAllowanceEditor } from "./CodexAllowanceEditor";
import { CodexAllowanceTable } from "./CodexAllowanceTable";
import { CodexReferenceDetails } from "./CodexReferenceDetails";
import { CodexForecastOverview } from "./CodexForecastOverview";
import { CodexDeadlineInput } from "./CodexDeadlineInput";
import { deadlineDraft } from "@/lib/codexDeadline";
import { PageTabs } from "@/components/ui/page-tabs";
import { Button } from "@/components/ui/button";
import type {
  CodexAllowanceCatalog,
  CodexAllowanceInfo,
  CodexAllowanceMode,
} from "@/types/codexAllowances";
import { accountError } from "@/lib/providerAccounts";
import type { CodexPlan, ProviderAccountView } from "@/types/providerAccounts";

type ForecastTab = "overview" | "target" | "details" | "config";
export function CodexReferencePanel({
  view,
  zh,
  locale,
  timeZone,
}: {
  view: ProviderAccountView;
  zh: boolean;
  locale: string;
  timeZone: string;
}) {
  const l = (a: string, b: string) => (zh ? a : b);
  const [plan, setPlan] = useState<CodexPlan>(
    view.account.codex?.plan ?? "plus",
  );
  const [model, setModel] = useState("");
  const [lookback, setLookback] = useState(0);
  const [pace, setPace] = useState("1");
  const [bucket, setBucket] = useState("codex");
  const [mode, setMode] = useState<CodexAllowanceMode | null>(null);
  const [editor, setEditor] = useState<{
    info: CodexAllowanceInfo;
    draft: CodexAllowanceCatalog;
  } | null>(null);
  const rules = useQuery({
    queryKey: ["codex-allowances"],
    queryFn: codexAllowancesApi.info,
    staleTime: 15_000,
    refetchInterval: 30_000,
  });
  const effectiveMode = mode ?? rules.data?.catalog.defaultMode ?? "modeldial";
  const quota =
    view.snapshot?.data.kind === "codexQuota" ? view.snapshot.data : undefined;
  const activeBucket = quota?.buckets.some((v) => v.id === bucket)
    ? bucket
    : (quota?.buckets[0]?.id ?? "codex");
  const officialReset =
    quota?.buckets
      .find((b) => b.id === activeBucket)
      ?.windows.find((w) => w.durationMinutes === 10080)?.resetsAt ?? null;
  const [deadline, setDeadline] = useState(() =>
    deadlineDraft(
      officialReset ?? Math.floor(Date.now() / 1000) + 86400,
      timeZone,
    ),
  );
  useEffect(
    () =>
      setDeadline((previous) =>
        previous.at == null ? previous : deadlineDraft(previous.at, timeZone),
      ),
    [timeZone],
  );
  const [tab, setTab] = useState<ForecastTab>("overview");
  const panelId = useId();
  const args: CodexReferenceArgs = {
    id: view.account.id,
    plan,
    model: model || undefined,
    bucket: activeBucket,
    lookbackMinutes: lookback,
    pace,
    mode: effectiveMode,
  };
  const query = useQuery({
    ...codexReferenceQuery(view, args, rules.data?.revision),
    enabled: !!rules.data,
  });
  const data = query.data;
  const plans =
    rules.data?.catalog.plans ??
    Object.entries(CODEX_PLANS).map(([id, p]) => ({
      id,
      name: p.name,
      monthlyFeeUsd: String(p.monthlyUsd),
      theoreticalMultiplier: p.multiplier,
    }));
  return (
    <section
      className="scope-card !p-4 space-y-3"
      aria-label={l(
        "订阅参考续航与每日预算",
        "Subscription reference forecast",
      )}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="font-semibold">
          {l("订阅参考续航与每日预算", "Subscription reference forecast")}
        </h2>
        <span className="text-caption text-fg-3">
          {plans.find((p) => p.id === plan)?.name} ·{" "}
          {effectiveMode === "modeldial"
            ? "ModelDial"
            : l("理论倍率", "Theoretical")}{" "}
          · {pace}×
        </span>
      </div>
      <PageTabs<ForecastTab>
        aria-label={l("订阅预测页签", "Subscription forecast tabs")}
        value={tab}
        onValueChange={setTab}
        idPrefix={panelId}
        controls={`${panelId}-panel`}
        items={[
          { value: "overview", label: l("概览", "Overview") },
          { value: "target", label: l("目标重置", "Target reset") },
          { value: "details", label: l("窗口与采样", "Windows & samples") },
          { value: "config", label: l("参考配置", "Reference rules") },
        ]}
      />
      {rules.error && (
        <p role="alert" className="text-caption text-danger-text">
          {String(rules.error)}
        </p>
      )}
      {query.isError && (
        <p role="alert" className="text-caption text-danger-text">
          {accountError(String(query.error), zh)}
        </p>
      )}
      <div
        role="tabpanel"
        id={`${panelId}-panel`}
        aria-labelledby={`${panelId}-${tab}`}
        className="space-y-4"
      >
        {tab !== "config" && (
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <label className="flex flex-col gap-1 text-caption text-fg-2">
              {l("预测模型", "Forecast model")}
              <OptionSelect
                aria-label={l("预测模型", "Forecast model")}
                className="scope-select w-full"
                value={model}
                onValueChange={(value) => setModel(value)}
              >
                <option value="">
                  {l("最近使用（自动）", "Most recent (auto)")}
                  {!model && data?.model ? ` · ${data.model}` : ""}
                </option>
                {Array.from(
                  new Set([...(data?.models ?? []), ...(model ? [model] : [])]),
                ).map((m) => (
                  <option key={m} value={m}>
                    {m}
                  </option>
                ))}
              </OptionSelect>
            </label>
            <label className="flex flex-col gap-1 text-caption text-fg-2">
              {l("消耗速率采样", "Burn-rate sampling")}
              <OptionSelect
                aria-label={l("消耗速率采样", "Burn-rate sampling")}
                className="scope-select w-full"
                value={lookback}
                onValueChange={(value) => setLookback(Number(value))}
              >
                <option value={0}>
                  {l("当前额度窗口", "Current quota window")}
                </option>
                <option value={15}>
                  {l("最近 15 分钟", "Last 15 minutes")}
                </option>
                <option value={60}>{l("最近 1 小时", "Last hour")}</option>
                <option value={1440}>
                  {l("最近 24 小时", "Last 24 hours")}
                </option>
              </OptionSelect>
            </label>
          </div>
        )}
        {tab !== "config" && (rules.isLoading || query.isLoading) && (
          <p className="text-caption text-fg-3">
            {l("正在计算参考预算…", "Calculating reference budget…")}
          </p>
        )}
        {tab !== "config" &&
          data?.sample.startDate != null &&
          (data.sample.durationSeconds ?? 0) > 0 && (
            <p className="text-caption text-fg-3">
              {l("有效采样", "Effective sample")}:{" "}
              {new Date(data.sample.startDate * 1000).toLocaleString(locale, {
                timeZone,
              })}{" "}
              —{" "}
              {new Date(data.asOf * 1000).toLocaleString(locale, { timeZone })}
              {l(
                "；不含本次重置前的数据，包含空闲时间。",
                "; excludes usage before this reset and includes idle time.",
              )}
            </p>
          )}
        {tab === "overview" && data && (
          <CodexForecastOverview
            data={data}
            view={view}
            args={args}
            revision={rules.data?.revision}
            speedRules={rules.data?.catalog.subscriptionSpeed}
            onModelChange={setModel}
            zh={zh}
            locale={locale}
          />
        )}
        {tab === "target" && (
          <>
            <CodexDeadlineInput
              draft={deadline}
              onChange={setDeadline}
              timeZone={timeZone}
              officialReset={officialReset}
              zh={zh}
            />
            {data && (
              <CodexForecastOverview
                data={data}
                view={view}
                args={args}
                revision={rules.data?.revision}
                speedRules={rules.data?.catalog.subscriptionSpeed}
                onModelChange={setModel}
                zh={zh}
                locale={locale}
                targetAt={deadline.at}
              />
            )}
          </>
        )}
        {tab === "details" && data && (
          <CodexReferenceDetails data={data} zh={zh} locale={locale} />
        )}
        {tab === "config" && (
          <>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <label className="flex flex-col gap-1 text-caption text-fg-2">
                {l("估算套餐", "Reference plan")}
                <OptionSelect
                  aria-label={l("估算套餐", "Reference plan")}
                  className="scope-select w-full"
                  value={plan}
                  onValueChange={(value) => setPlan(value as CodexPlan)}
                >
                  {plans.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name} · ${p.monthlyFeeUsd}/mo ·{" "}
                      {p.theoreticalMultiplier}×
                    </option>
                  ))}
                </OptionSelect>
              </label>
              <label className="flex flex-col gap-1 text-caption text-fg-2">
                {l("使用强度", "Usage intensity")}
                <OptionSelect
                  aria-label={l("使用强度", "Usage intensity")}
                  className="scope-select w-full"
                  value={pace}
                  onValueChange={(value) => setPace(value)}
                >
                  {["0.25", "0.5", "1", "1.5", "2", "4"].map((p) => (
                    <option key={p} value={p}>
                      {p}×
                    </option>
                  ))}
                </OptionSelect>
              </label>
              <label className="flex flex-col gap-1 text-caption text-fg-2">
                {l("参考额度基准", "Allowance basis")}
                <OptionSelect
                  aria-label={l("参考额度基准", "Allowance basis")}
                  className="scope-select w-full sm:max-w-md"
                  value={effectiveMode}
                  onValueChange={(value) =>
                    setMode(value as CodexAllowanceMode)
                  }
                >
                  <option value="modeldial">
                    {l(
                      "ModelDial 优先；缺失项按配置回退",
                      "ModelDial first; configured fallback for missing samples",
                    )}
                  </option>
                  <option value="theoretical">
                    {l(
                      "Plus 样本 × 理论倍率",
                      "Plus estimate × theoretical multiplier",
                    )}
                  </option>
                </OptionSelect>
              </label>
              {quota && quota.buckets.length > 1 && (
                <label className="flex flex-col gap-1 text-caption text-fg-2">
                  {l("官方额度桶", "Provider quota bucket")}
                  <OptionSelect
                    aria-label={l("官方额度桶", "Provider quota bucket")}
                    className="scope-select w-full"
                    value={activeBucket}
                    onValueChange={(value) => setBucket(value)}
                  >
                    {quota.buckets.map((b) => (
                      <option key={b.id} value={b.id}>
                        {b.name ?? b.id}
                      </option>
                    ))}
                  </OptionSelect>
                </label>
              )}
            </div>
            <p className="text-caption text-fg-3">
              {l(
                "月费只表示订阅价格，参考额度按模型单独配置。Plus 有 5h 与每周，Pro 仅每周；理论倍率为相对 Plus 的 5×／10×／25×，ModelDial 样本不必严格符合该比例。",
                "Monthly fees are subscription prices, separate from model allowances. Plus has 5h and weekly references; Pro weekly only. Theoretical Plus multipliers are 5×/10×/25×; ModelDial estimates can differ.",
              )}
            </p>
            <div className="flex flex-wrap gap-2">
              <Button
                size="compact"
                variant="neutral"
                disabled={!rules.data}
                onClick={() =>
                  rules.data &&
                  setEditor({ info: rules.data, draft: rules.data.catalog })
                }
              >
                {l("编辑本地配置", "Edit local configuration")}
              </Button>
              {rules.data?.file.catalog && (
                <Button
                  size="compact"
                  variant="neutral"
                  onClick={() =>
                    rules.data?.file.catalog &&
                    setEditor({
                      info: rules.data,
                      draft: rules.data.file.catalog,
                    })
                  }
                >
                  {l("从文件载入", "Load from file")}
                </Button>
              )}
            </div>
            {rules.data?.file.error && (
              <p role="alert" className="text-caption text-danger-text">
                {rules.data.file.error}
              </p>
            )}
            {rules.data && (
              <>
                <p className="text-caption text-fg-3 break-all">
                  {rules.data.path}
                </p>
                <p className="text-caption text-fg-3">
                  {l(
                    "外部文件是草稿，载入并校验应用后生效。更改参考档位和规则会保留手动订阅资料与历史费用。",
                    "External files are drafts until loaded, validated and applied. Changing reference plans or rules keeps manual billing and historical costs.",
                  )}
                </p>
                <CodexAllowanceTable
                  info={rules.data}
                  mode={effectiveMode}
                  zh={zh}
                  locale={locale}
                />
              </>
            )}
          </>
        )}
      </div>
      {editor && (
        <CodexAllowanceEditor
          info={editor.info}
          draft={editor.draft}
          zh={zh}
          onClose={() => setEditor(null)}
        />
      )}
    </section>
  );
}
