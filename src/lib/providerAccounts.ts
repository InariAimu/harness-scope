import type { QuotaWindow } from "@/types/providerAccounts";

export const ACCOUNT_PROVIDER_NAMES = {
  deepseek: "DeepSeek",
  "opencode-go": "OpenCode Go",
  "openai-codex": "OpenAI / Codex",
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
  if (message.includes("Codex executable not found"))
    return "未找到 Codex 程序，请在账号设置中填写 codex.exe 的绝对路径。";
  if (message.includes("Codex home directory not found"))
    return "Codex 数据目录不存在，请检查账号设置。";
  if (message.includes("local ChatGPT login"))
    return "无法读取 Codex 订阅，请先在对应 Codex 中使用 ChatGPT 账号登录，并检查 Codex 版本。";
  if (message.includes("Codex paths"))
    return "Codex 程序与数据目录须填写绝对路径，或留空自动检测。";
  if (message.includes("401")) return "API Key 无效或已过期，请检查凭据。";
  if (message.includes("403"))
    return "此 Key 没有查询权限，或账号没有有效订阅。";
  if (message.includes("429")) return "供应商请求频率受限，请稍后刷新。";
  if (message.includes("already linked"))
    return "所选本机路由已关联其他账号，请先解除原关联。";
  if (message.includes("Renewal date"))
    return "下次续费日期须晚于本期开始日期。";
  if (message.includes("timed out")) return "供应商暂时无法连接，请稍后刷新。";
  if (message.includes("Refresh interval"))
    return "刷新间隔须为 0（关闭）或 15–86400 秒。";
  if (message.toLowerCase().includes("credential"))
    return "无法读取本机凭据，请重新保存 API Key。";
  return message;
}
