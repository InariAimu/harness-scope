import type { QuotaWindow } from "@/types/providerAccounts";

export const ACCOUNT_PROVIDER_NAMES = {
  deepseek: "DeepSeek",
  "opencode-go": "OpenCode Go",
};
export const routeKey = (route: { harness: string; provider: string }) =>
  JSON.stringify([route.harness, route.provider]);

export function quotaDisplay(window: QuotaWindow, now: number) {
  return {
    remaining:
      window.status === "rate-limited"
        ? 0
        : Math.max(0, Math.min(100, 100 - window.usedPercent)),
    used: Math.max(0, Math.min(100, window.usedPercent)),
    expired: window.resetsAt <= now,
    limited: window.status === "rate-limited" || window.usedPercent >= 100,
  };
}
export function nativeMoney(
  value: string | null | undefined,
  currency: string,
  locale: string,
) {
  if (value == null || value.trim() === "" || !Number.isFinite(Number(value)))
    return "—";
  try {
    return new Intl.NumberFormat(locale, {
      style: "currency",
      currency,
      minimumFractionDigits: 2,
      maximumFractionDigits: 8,
    }).format(Number(value));
  } catch {
    return `${value} ${currency}`;
  }
}
export function accountError(message: string, zh: boolean) {
  if (!zh) return message;
  if (message.includes("401")) return "API Key 无效或已过期，请检查凭据。";
  if (message.includes("403"))
    return "此 Key 没有查询权限，或账号没有有效订阅。";
  if (message.includes("429")) return "供应商请求频率受限，请稍后刷新。";
  if (message.includes("already linked"))
    return "所选本机路由已关联其他账号，请先解除原关联。";
  if (message.includes("Renewal date"))
    return "下次续费日期须晚于本期开始日期。";
  if (message.includes("timed out")) return "供应商暂时无法连接，请稍后刷新。";
  return message;
}
