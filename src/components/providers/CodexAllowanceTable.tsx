import { useUsageNumberFormat } from "@/hooks/useUsageNumberFormat";
import type {
  CodexAllowanceInfo,
  CodexAllowanceMode,
} from "@/types/codexAllowances";
import { nativeMoney } from "@/lib/providerAccounts";
import { ProviderHelp } from "./ProviderHelp";
export function CodexAllowanceTable({
  info,
  mode,
  zh,
  locale,
}: {
  info: CodexAllowanceInfo;
  mode: CodexAllowanceMode;
  zh: boolean;
  locale: string;
}) {
  const l = (a: string, b: string) => (zh ? a : b);
  const formatQuantity = useUsageNumberFormat();
  const money = (s: string) =>
    nativeMoney(Number(s).toPrecision(4), "USD", locale);
  const basisLabel = (basis: string) =>
    basis === "theoretical"
      ? l("Plus 理论倍率推算", "Plus theoretical multiplier")
      : basis === "weekly_division"
        ? l(
            "由每周值按配置窗口数推算",
            "Derived from weekly estimate / configured divisor",
          )
        : l("ModelDial 估算样本", "ModelDial estimate");
  return (
    <section className="text-caption">
      <h3 className="font-medium">
        {l("分模型每周参考额度表", "Model-specific weekly estimates")}
        <ProviderHelp label={l("参考额度表说明", "About weekly estimates")}>
          {l(
            "同一套餐各模型共享额度，不叠加。API 等值（USD）用于参考预算；来源 Token 数含缓存，仅供对照。",
            "Models share the same plan quota; do not add them together. API equivalents (USD) drive reference budgets; source tokens include cache and are comparison figures only.",
          )}
        </ProviderHelp>
      </h3>
      <p className="text-fg-3 my-2">
        <a
          className="underline"
          href={info.catalog.sourceUrl}
          target="_blank"
          rel="noreferrer"
        >
          ModelDial
        </a>{" "}
        · {info.catalog.checkedAt}
      </p>
      <div className="overflow-x-auto">
        <table className="w-full text-left">
          <thead>
            <tr>
              <th className="p-2">{l("模型", "Model")}</th>
              {info.catalog.plans.map((p) => (
                <th className="p-2" key={p.id}>
                  {p.name}
                  <span className="block text-fg-3 font-normal">
                    ${p.monthlyFeeUsd}/mo · {p.theoreticalMultiplier}×
                  </span>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {info.resolved[mode].map((m) => (
              <tr key={m.id} className="border-t border-border">
                <td className="p-2 font-medium" title={m.notes}>
                  {m.name}
                </td>
                {info.catalog.plans.map((p) => {
                  const v = m.plans[p.id];
                  return (
                    <td key={p.id} className="p-2">
                      {v ? (
                        <>
                          <span>≈{money(v.apiEquivalentUsd)}</span>
                          <span className="block text-fg-3">
                            {v.tokens
                              ? formatQuantity(v.tokens)
                              : "—"}{" "}
                            Tokens
                          </span>
                          <span className="block text-fg-3">
                            {basisLabel(v.basis)}
                          </span>
                        </>
                      ) : (
                        l("无可比样本", "No comparable sample")
                      )}
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}
