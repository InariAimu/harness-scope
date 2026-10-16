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
  const remove = useDeleteModelPricing();
  const [edit, setEdit] = useState<ModelPricing | null>(null);
  const [picker, setPicker] = useState(false);
  const [search, setSearch] = useState("");
  return (
    <div className="space-y-5">
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
        USD / 1M tokens ·{" "}
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
                `${p.modelId} ${p.displayName}`
                  .toLowerCase()
                  .includes(search.toLowerCase()),
              )
              .map((p) => (
                <tr key={p.modelId} className="border-t border-border">
                  <td className="px-3 py-2">
                    <div>{p.displayName}</div>
                    <div className="text-caption text-fg-3">{p.modelId}</div>
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
                      ${n}
                    </td>
                  ))}
                  <td className="px-3">
                    <button
                      aria-label={`Edit ${p.modelId}`}
                      className="p-1"
                      onClick={() => setEdit(p)}
                    >
                      <Pencil size={14} />
                    </button>
                    <button
                      aria-label={`Delete ${p.modelId}`}
                      className="p-1"
                      onClick={() =>
                        remove.mutate(p.modelId, {
                          onError: (e) => toast.error(String(e)),
                        })
                      }
                    >
                      <Trash2 size={14} />
                    </button>
                  </td>
                </tr>
              ))}
          </tbody>
        </table>
      </div>
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
