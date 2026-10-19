import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Globe, Plus, Pencil, Trash2 } from "lucide-react";
import { useModelPricing, useDeleteModelPricing } from "@/lib/query/usage";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import type { ModelPricing } from "@/types/usage";
import { PricingEditModal } from "./PricingEditModal";
import { ModelsDevPickerDialog } from "./ModelsDevPickerDialog";
import { ModelsDevAutoSyncPanel } from "./ModelsDevAutoSyncPanel";
import { toast } from "@/lib/toast";
import { getUsageProviderLabel } from "./providerLabel";
import {
  PricingSettingsPanel,
  usePricingSettings,
} from "./PricingSettingsPanel";
const empty: ModelPricing = {
  modelId: "",
  displayName: "",
  inputCostPerMillion: "0",
  outputCostPerMillion: "0",
  cacheReadCostPerMillion: "0",
  cacheCreationCostPerMillion: "0",
};
export function PricingConfigPanel() {
  const { t } = useTranslation();
  const query = useModelPricing();
  const settings = usePricingSettings();
  const remove = useDeleteModelPricing();
  const [edit, setEdit] = useState<ModelPricing | null>(null);
  const [picker, setPicker] = useState(false);
  const [search, setSearch] = useState("");
  return (
    <div className="space-y-5">
      <PricingSettingsPanel />
      <ModelsDevAutoSyncPanel />
      <div className="flex items-center gap-2">
        <Input
          aria-label="Search pricing"
          className="max-w-xs"
          placeholder={t("common.search", "搜索")}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        <span className="flex-1" />
        <Button
          variant="neutral"
          size="compact"
          onClick={() => setPicker(true)}
        >
          <Globe size={14} />
          models.dev
        </Button>
        <Button size="compact" onClick={() => setEdit(empty)}>
          <Plus size={14} />
          {t("common.add", "添加")}
        </Button>
      </div>
      <p className="text-caption text-fg-2">
        {t(
          "scope.pricing.nativeUnits",
          "原币 / 1M Tokens；DeepSeek 按空闲 / 高峰显示",
        )}{" "}
        ·{" "}
        {t(
          "scope.pricingNote",
          "费用保存实际价表来源和单价快照。更新价表仅补齐未知费用。",
        )}
      </p>
      <div className="overflow-auto rounded-panel border border-border">
        <table className="w-full text-left text-body">
          <thead className="bg-sidebar text-fg-2">
            <tr>
              {[
                t("usage.model", "模型"),
                t("usage.provider", "供应商"),
                t("scope.source", "来源"),
                t("usage.inputTokens", "输入"),
                t("usage.outputTokens", "输出"),
                t("usage.cacheRead", "缓存读取"),
                t("usage.cacheCreation", "缓存写入"),
                "",
              ].map((x, i) => (
                <th key={i} className="px-3 py-2 font-medium">
                  {x}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {query.data
              ?.filter((p) =>
                `${p.modelId} ${p.displayName} ${p.provider ?? ""} ${p.provider ? getUsageProviderLabel(p.provider, t).label : "models.dev"}`
                  .toLowerCase()
                  .includes(search.toLowerCase()),
              )
              .map((p) => (
                <tr
                  key={`${p.provider ?? ""}/${p.modelId}`}
                  className="border-t border-border"
                >
                  <td className="px-3 py-2">
                    <div>{p.displayName}</div>
                    <div className="text-caption text-fg-3">{p.modelId}</div>
                  </td>
                  <td
                    className="px-3 text-caption"
                    title={p.provider ?? undefined}
                  >
                    {p.provider
                      ? getUsageProviderLabel(p.provider, t).label
                      : t("scope.pricing.generic", "通用模型参考价")}
                    {p.billingBasis === "subscription_usage" && (
                      <div className="text-fg-3">
                        {t("scope.pricing.subscriptionUsage", "套餐额度折算")}
                      </div>
                    )}
                  </td>
                  <td className="px-3 text-caption" title={p.sourceKey}>
                    {p.source || "unknown"}
                  </td>
                  {[
                    p.inputCostPerMillion,
                    p.outputCostPerMillion,
                    p.cacheReadCostPerMillion,
                    p.cacheCreationCostPerMillion,
                  ].map((n, i) => (
                    <td key={i} className="px-3 tabular-nums">
                      <div>
                        {p.currency === "CNY"
                          ? "¥"
                          : p.currency && p.currency !== "USD"
                            ? `${p.currency} `
                            : "$"}
                        {n}
                        {p.pricingRule
                          ? ` / ${p.currency === "CNY" ? "¥" : "$"}${Number(n) * 2}`
                          : ""}
                      </div>
                      {p.currency &&
                        p.currency !== "USD" &&
                        Number(settings.data?.exchangeRates[p.currency]) >
                          0 && (
                          <div className="text-caption text-fg-3">
                            ≈$
                            {(
                              Number(n) /
                              Number(settings.data?.exchangeRates[p.currency])
                            ).toFixed(6)}
                            {p.pricingRule
                              ? ` / $${((Number(n) * 2) / Number(settings.data?.exchangeRates[p.currency])).toFixed(6)}`
                              : ""}
                          </div>
                        )}
                    </td>
                  ))}
                  <td className="px-3">
                    {!p.pricingRule && (
                      <button
                        aria-label={`Edit ${p.modelId}`}
                        className="p-1"
                        onClick={() => setEdit(p)}
                      >
                        <Pencil size={14} />
                      </button>
                    )}
                    {!p.pricingRule && (
                      <button
                        aria-label={`Delete ${p.modelId}`}
                        className="p-1"
                        onClick={() =>
                          remove.mutate(
                            { modelId: p.modelId, provider: p.provider },
                            {
                              onError: (e) => toast.error(String(e)),
                            },
                          )
                        }
                      >
                        <Trash2 size={14} />
                      </button>
                    )}
                  </td>
                </tr>
              ))}
          </tbody>
        </table>
      </div>
      <p className="text-caption text-fg-2">
        {t(
          "scope.pricing.officialNote",
          "DeepSeek 官方价表按日志时间估算，统计时区不影响计价时段。当前为 Token 费用，Search / Fetch 工具费用未统计。",
        )}{" "}
        <a
          className="text-link"
          href="https://api-docs.deepseek.com/zh-cn/quick_start/pricing/"
          target="_blank"
          rel="noreferrer"
        >
          {t("scope.pricing.officialLink", "官方计价说明")}
        </a>
      </p>
      <p className="text-caption text-fg-2">
        {t(
          "scope.pricing.goNote",
          "OpenCode Go 按 USD 计算套餐额度消耗，不含月费；高峰为 UTC 工作日 01–04、06–10 点，不套用中国节假日。未配置的供应商使用 models.dev 模型参考价，不代表实际账单。",
        )}{" "}
        <a
          className="text-link"
          href="https://opencode.ai/v2/docs/console/go/"
          target="_blank"
          rel="noreferrer"
        >
          OpenCode Go
        </a>
      </p>
      {edit && (
        <PricingEditModal
          open
          model={edit}
          isNew={!edit.modelId}
          onClose={() => setEdit(null)}
        />
      )}
      <ModelsDevPickerDialog
        open={picker}
        onClose={() => setPicker(false)}
        onImported={() => setPicker(false)}
      />
    </div>
  );
}
