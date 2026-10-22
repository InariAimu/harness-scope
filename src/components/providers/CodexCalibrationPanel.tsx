import type { CodexReference } from "@/types/providerAccounts";
import { codexWindowLabel } from "@/lib/codexSubscription";
import {
  forecastDuration,
  forecastNumber,
  forecastReason,
} from "@/lib/goForecast";
import { nativeMoney } from "@/lib/providerAccounts";
export function CodexCalibrationPanel({
  data,
  zh,
  locale,
}: {
  data: CodexReference;
  zh: boolean;
  locale: string;
}) {
  const l = (a: string, b: string) => (zh ? a : b);
  const calibration = data.calibration;
  const n = (v: string | undefined) =>
    v == null ? "—" : forecastNumber(v, locale);
  const money = (v: string | undefined) =>
    v == null ? "—" : nativeMoney(Number(v).toPrecision(4), "USD", locale);
  const reason = (r: string | null) => {
    const texts: Record<string, [string, string]> = {
      insufficient_quota_history: [
        "需要至少两次同账号、同窗口的有效额度快照，间隔至少 1 分钟。自动刷新会继续积累。",
        "Need two snapshots for the same account/window spanning one minute. Auto refresh keeps collecting.",
      ],
      no_quota_growth: [
        "此区间额度增长不足 1 个百分点，暂不校准，不推算为无限可用。",
        "Quota growth is below one percentage point; no calibration or unlimited lifetime is inferred.",
      ],
      no_linked_calibration_usage: [
        "额度有变化，但没有同期可归因的关联计费请求，暂不推算当前模型强度。",
        "Quota changed without attributable linked priced requests; model intensity remains unknown.",
      ],
      unpriced_calibration: [
        "同期有未定价或缺少参考额度的模型，不能可靠分配额度变化。",
        "Some concurrent models lack prices or allowance estimates; quota changes cannot be reliably allocated.",
      ],
    };
    return r && texts[r] ? texts[r][zh ? 0 : 1] : forecastReason(r, zh);
  };
  return (
    <section className="rounded-panel border border-border p-3 space-y-3">
      <h3 className="font-medium text-caption">
        {l("按官方额度变化自动校准", "Calibration from provider quota changes")}
      </h3>
      <p className="text-caption text-fg-3">
        {l(
          "校准始终针对当前登录账号；上方套餐切换是参考情景，不表示账号实际升级。混合模型先按各自参考额度归一化，再与官方变化对齐。",
          "Calibration always targets the signed-in account. Switching the reference plan is a scenario, not an account upgrade. Mixed models are normalized by their own reference allowances before fitting quota changes.",
        )}
      </p>
      {!calibration.identityVerified && (
        <p className="text-caption text-fg-3">
          {l(
            "等待可验证账号身份的额度快照；不会按邮箱猜测同一订阅。",
            "Waiting for a quota snapshot with a verifiable account identity; email alone does not establish the same subscription.",
          )}
        </p>
      )}
      {calibration.identityVerified && !calibration.windows.length && (
        <p className="text-caption text-fg-3">
          {l(
            "等待此额度桶的有效窗口快照。",
            "Waiting for a valid window snapshot for this bucket.",
          )}
        </p>
      )}
      {calibration.currentWindowTarget && (
        <p className="rounded-control bg-subtle p-3 text-caption font-medium">
          {l(
            "同时满足当前窗口的均匀强度上限",
            "Uniform intensity allowed by all current windows",
          )}
          : ≈{n(calibration.currentWindowTarget.intensity)}× ·{" "}
          {l("约束来自", "constrained by")}{" "}
          {codexWindowLabel(
            calibration.currentWindowTarget.durationMinutes,
            zh,
          )}{" "}
          · {l("最早重置后重新计算", "recalculate after the first reset")}
        </p>
      )}
      <div
        className={`grid grid-cols-1 ${calibration.windows.length > 1 ? "sm:grid-cols-2" : ""} gap-3`}
      >
        {calibration.windows.map((w) => (
          <section
            key={`${w.id}-${w.durationMinutes}`}
            className="rounded-control border border-border p-3 space-y-2 text-caption"
          >
            <h4 className="font-medium">
              {codexWindowLabel(w.durationMinutes, zh)}
            </h4>
            {w.observedPercentPerHour != null && (
              <p>
                {l("账号整体观察速率", "Observed account-wide rate")}: ≈
                {n(w.observedPercentPerHour)}{" "}
                {l("百分点/小时", "percentage points/hour")}
              </p>
            )}
            {w.status === "ready" ? (
              <>
                <p>
                  {l("所选模型校准速率", "Calibrated rate for selected model")}:
                  ≈{n(w.modelPercentPerHour)}{" "}
                  {l("百分点/小时", "percentage points/hour")} · {data.pace}×{" "}
                  {l("当前设置", "current setting")}
                </p>
                <strong className="block">
                  {w.coversReset
                    ? l(
                        "按校准速率可覆盖到重置",
                        "Calibrated pace covers reset",
                      )
                    : l(
                        `约 ${forecastDuration(w.runtimeSeconds!, zh)} 后耗尽（校准估算）`,
                        `Exhausts in about ${forecastDuration(w.runtimeSeconds!, zh)} (calibrated estimate)`,
                      )}
                </strong>
                <p className="font-medium">
                  {l(
                    "均匀用到重置需要",
                    "To use the remainder evenly before reset",
                  )}
                  : ≈{n(w.requiredIntensity)}×{" "}
                  {l("采样强度", "sampled intensity")}
                </p>
                <p>
                  ≈{n(w.requiredPercentPerHour)}{" "}
                  {l("百分点/小时", "percentage points/hour")} ·{" "}
                  {money(w.requiredUsdPerHour)} /{" "}
                  {l("小时 API 折算", "hour API equivalent")} ·{" "}
                  {n(w.requiredRequestsPerHour)} {l("次/小时", "requests/hour")}
                </p>
              </>
            ) : (
              <p className="text-fg-3">{reason(w.reason)}</p>
            )}
            {w.spanSeconds != null && (
              <p className="text-fg-3">
                {w.observations} {l("次快照", "snapshots")} ·{" "}
                {forecastDuration(w.spanSeconds, zh)} ·{" "}
                {l("额度增长", "quota growth")} {n(w.deltaPercent)}{" "}
                {l("百分点", "percentage points")}
                {w.linkedRequests != null
                  ? ` · ${w.linkedRequests} ${l("关联请求", "linked requests")}`
                  : ""}
              </p>
            )}
            {w.modelCount != null && (
              <p className="text-fg-3">
                {w.modelCount} {l("种模型参与归一化", "models normalized")} ·{" "}
                {l("同期 API 折算", "Concurrent API equivalent")}{" "}
                {money(w.linkedUsd)}
              </p>
            )}
          </section>
        ))}
      </div>
      <p className="text-caption text-fg-3">
        {l(
          "按共用订阅计算，采样包含所有已同步设备的关联路由。假设额度变化主要来自这些日志；尚未同步的用量、服务端延迟、缓存组成或模型切换会影响归因。账号或窗口重置后重新采样；同一窗口内额度回退的读数会被筛除。目标倍率相对原始采样速率，不随强度滑档改变；多个窗口需同时满足，不能把各窗口或模型额度相加。",
          "Assumes a shared subscription and includes linked routes from all synced devices. Unsynced usage, billing lag, cache mix or model switches affect attribution. Account/window changes restart sampling; regressing counters within a window are filtered out. Target multipliers are relative to the original sampled pace. All windows constrain usage; quotas are not additive.",
        )}
      </p>
    </section>
  );
}
