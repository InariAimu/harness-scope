export function forecastReason(reason: string | null, zh: boolean) {
  const messages: Record<string, [string, string]> = {
    unknown_speed_baseline: [
      "校准区间包含未知请求档位，暂用参考估算；缺少日志设置的历史无法推断模式。",
      "Calibration contains unknown request tiers; reference estimates apply. Missing historical settings cannot be inferred.",
    ],
    unsupported_fast_model: [
      "本地官方规则未确认此模型支持 Fast，暂不套用倍率。",
      "Local official rules do not confirm Fast support for this model.",
    ],
    insufficient_quota_history: [
      "尚无足够的同账号额度历史，暂不能按校准数据评估速度模式。",
      "Not enough same-account quota history to evaluate speed modes from calibration.",
    ],
    no_quota_growth: [
      "额度变化不足一个百分点，暂不能校准速度模式。",
      "Quota changed by less than one percentage point; speed calibration is unavailable.",
    ],
    invalid_target_time: [
      "请输入有效且晚于当前时刻的目标时间。",
      "Enter a valid target time after the current time.",
    ],
    target_after_reset: [
      "目标晚于官方周重置时间，当前余量无法跨重置延用，请选择官方重置之前的时间。",
      "Target is after the official weekly reset; choose an earlier time for the current remainder.",
    ],
    other_window_unknown: [
      "其他窗口暂不可预测，目标强度可能仍受其限制。",
      "Another quota window is unavailable and may constrain this target.",
    ],
    other_window_limits_target: [
      "此均匀强度会先触及其他窗口上限，需要调整使用节奏。",
      "This uniform intensity reaches another window limit first; adjust the schedule.",
    ],
    window_resets_before_target: [
      "其他窗口会先于目标重置，届时应重新评估强度；未预估未来补充额度。",
      "Another window resets before the target; recalculate then. Future replenishments are excluded.",
    ],
    no_model_budget: [
      "本地配置没有此模型／档位的参考额度，请补充配置。",
      "No local allowance for this model/plan; update the configuration.",
    ],
    unsupported_bucket: [
      "本地配置未关联此额度桶，暂不估算。",
      "This quota bucket is not mapped by local allowance rules.",
    ],
    no_links: ["先关联本机用量路由。", "Link local usage routes first."],
    no_model: [
      "此采样区间没有可识别的最近模型；可手动选择模型。",
      "No recognized latest model in this interval; select a model.",
    ],
    free_model: [
      "当前本地表将此模型标为免费，不计算额度续航。",
      "This model is free in the local table; no allowance lifetime is projected.",
    ],
    unpriced_samples: [
      "样本含未公布单价，无法完整估算消耗速度。",
      "Some sample rates are unavailable; the full burn rate is unknown.",
    ],
    insufficient_samples: [
      "样本不足：至少需要 3 次请求，且首末样本间隔至少 1 分钟。可扩大采样区间。",
      "Need at least 3 requests spanning 1 minute. Try a longer sampling interval.",
    ],
    zero_rate: [
      "未观察到可计算的额度消耗，无法推算可用时长。",
      "No measurable allowance consumption; lifetime is unknown.",
    ],
    no_quota: [
      "先刷新官方额度，再预测剩余时长。",
      "Refresh official quota before predicting remaining time.",
    ],
    stale_quota: [
      "官方额度快照超过 10 分钟或时间无效，请刷新额度。",
      "Official quota is older than 10 minutes or has an invalid timestamp; refresh it.",
    ],
    expired_window: [
      "窗口已到期或重置时间无效，请刷新官方额度。",
      "The window expired or has an invalid reset time; refresh official quota.",
    ],
    unknown_quota: [
      "官方窗口状态未知，暂不预测。",
      "Provider window status is unknown; prediction unavailable.",
    ],
    unpriced_since_snapshot: [
      "快照之后有未能折算的本机请求，无法可靠更新剩余额度。",
      "Some local usage since the snapshot is unpriced; remaining quota cannot be updated reliably.",
    ],
    precision_limit: [
      "规则产生的预算超出可预测范围，请检查单价。",
      "The configured rates produce an unsupported budget; check the prices.",
    ],
  };
  return reason ? (messages[reason]?.[zh ? 0 : 1] ?? reason) : "";
}
export function forecastDuration(seconds: number, zh: boolean) {
  if (!Number.isFinite(seconds) || seconds < 0) return "—";
  if (seconds < 60)
    return zh ? `${Math.ceil(seconds)} 秒` : `${Math.ceil(seconds)}s`;
  const minutes = Math.ceil(seconds / 60);
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor((minutes % 1440) / 60);
  const rest = minutes % 60;
  return [
    days ? `${days}${zh ? "天" : "d"}` : "",
    hours ? `${hours}${zh ? "小时" : "h"}` : "",
    !days && rest ? `${rest}${zh ? "分钟" : "m"}` : "",
  ]
    .filter(Boolean)
    .join(" ");
}
export function forecastNumber(value: string, locale: string) {
  const n = Number(value);
  if (!Number.isFinite(n)) return "—";
  if (n > 0 && n < 0.01) return n.toPrecision(2);
  return n.toLocaleString(locale, { maximumFractionDigits: 2 });
}
