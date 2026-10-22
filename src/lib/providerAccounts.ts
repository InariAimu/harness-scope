import type { QuotaWindow, ProviderAccountView, AccountProvider, CustomUsageItem } from "@/types/providerAccounts";

export const ACCOUNT_PROVIDER_NAMES = {
  deepseek: "DeepSeek",
  "opencode-go": "OpenCode Go",
  "openai-codex": "OpenAI / Codex",
};
export const isCustomProvider = (provider: string) => provider === "custom" || provider.startsWith("custom:");
export function accountProviderName(provider: AccountProvider, accounts: ProviderAccountView[] = [], zh = true) {
  if (isCustomProvider(provider)) return accounts.find(v => v.account.provider === provider)?.account.name ?? (zh ? "自定义供应商" : "Custom provider");
  return ACCOUNT_PROVIDER_NAMES[provider as keyof typeof ACCOUNT_PROVIDER_NAMES];
}
export function accountCanRefresh(view: ProviderAccountView) {
  return view.canRefresh ?? (view.account.custom
    ? view.account.custom.enabled && (!view.account.custom.script.includes("{{apiKey}}") || view.keyConfigured)
    : view.account.provider === "openai-codex" || view.keyConfigured);
}
export function customUsageValue(value: string | null, unit: string | null, locale: string) {
  if (value == null) return "—";
  if (unit && /^[A-Z]{3}$/.test(unit)) return nativeMoney(value, unit, locale);
  return `${Number(value).toLocaleString(locale, { maximumFractionDigits: 8 })}${unit ? ` ${unit}` : ""}`;
}
export function customRemainingPercent(item: CustomUsageItem, now: number) {
  if (item.usedPercent == null || (item.resetsAt != null && item.resetsAt <= now)) return null;
  return Math.max(0, Math.min(100, 100 - Number(item.usedPercent)));
}
/** Raw IDs stay the filter keys. Cross-tool aggregates only alias an unambiguous name. */
export function customProviderName(accounts: ProviderAccountView[], provider: string | undefined, harness?: string) {
  if (!provider) return undefined;
  const matches = accounts.filter(v => isCustomProvider(v.account.provider) && v.account.links.some(l => l.provider === provider && (!harness || harness === "all" || l.harness === harness)));
  return matches.length === 1 ? matches[0].account.name : undefined;
}
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
  if (message.includes("Custom query script failed")) return "查询脚本执行失败，请检查语法、同步返回值，或是否超出执行限制。";
  if (message.includes("configured base URL origin")) return "请求地址必须与 Base URL 使用相同的协议、域名和端口。";
  if (message.includes("Custom query reports an invalid")) return "查询结果报告账号无效或不可用。";
  if (message.includes("Custom query returned no usage values")) return "查询结果没有有效的余额、用量或百分比，请检查提取字段。";
  if (message.includes("stable id")) return "返回多条结果时，每条必须填写独立且稳定的 id。";
  if (message.includes("Duplicate custom query item")) return "查询结果中存在重复的 id。";
  if (message.includes("Invalid custom query amount")) return "用量数值无效，请返回数字或十进制字符串。";
  if (message.includes("Custom usage query is disabled")) return "此供应商的自定义用量查询已关闭。";
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
    return "所选路由已关联其他账号，请先解除原关联。";
  if (message.includes("Renewal date"))
    return "下次续费日期须晚于本期开始日期。";
  if (message.includes("timed out")) return "供应商暂时无法连接，请稍后刷新。";
  if (message.includes("Refresh interval"))
    return "刷新间隔须为 0（关闭）或 15–86400 秒。";
  if (message.toLowerCase().includes("credential"))
    return "无法读取本机凭据，请重新保存 API Key。";
  return message;
}
