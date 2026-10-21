import type { QuotaWindow } from "@/types/providerAccounts";

export function quotaWindowShortLabel(window: QuotaWindow, zh: boolean) {
  if (window.id === "weekly" || window.durationMinutes === 10080)
    return zh ? "周" : "7d";
  if (window.id === "monthly") return zh ? "月" : "mo";
  const minutes = window.durationMinutes ?? (window.id === "rolling" ? 300 : 0);
  if (!minutes) return "—";
  if (minutes % 1440 === 0) return `${minutes / 1440}d`;
  if (minutes % 60 === 0) return `${minutes / 60}h`;
  return `${minutes}m`;
}

export function quotaCountdown(resetsAt: number, now: number) {
  if (!Number.isFinite(resetsAt) || resetsAt <= now) return "—";
  const minutes = Math.ceil((resetsAt - now) / 60);
  if (minutes >= 1440)
    return `${Math.floor(minutes / 1440)}d${Math.floor((minutes % 1440) / 60)}h`;
  if (minutes >= 60) return `${Math.floor(minutes / 60)}h${minutes % 60}m`;
  return `${minutes}m`;
}

export function snapshotAge(
  fetchedAt: number | undefined,
  now: number,
  zh: boolean,
) {
  if (fetchedAt == null) return zh ? "未查询" : "Not fetched";
  const minutes = Math.max(0, Math.floor((now - fetchedAt) / 60));
  if (!minutes) return zh ? "刚刚" : "Just now";
  if (minutes < 60) return zh ? `${minutes}分钟前` : `${minutes}m ago`;
  if (minutes < 1440) {
    const hours = Math.floor(minutes / 60);
    return zh ? `${hours}小时前` : `${hours}h ago`;
  }
  const days = Math.floor(minutes / 1440);
  return zh ? `${days}天前` : `${days}d ago`;
}
