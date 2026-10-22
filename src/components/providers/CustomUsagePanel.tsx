import { customRemainingPercent, customUsageValue } from "@/lib/providerAccounts";
import type { CustomUsage } from "@/types/providerAccounts";

export function CustomUsagePanel({ data, zh, locale, timezone }: { data: CustomUsage; zh: boolean; locale: string; timezone: string }) {
  const now = Date.now() / 1000;
  return <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
    {data.items.map(item => {
      const percent = customRemainingPercent(item, now);
      const expired = item.resetsAt != null && item.resetsAt <= now;
      return <section className="scope-card !p-4 space-y-2 min-w-0" key={item.id}>
        <p className="text-caption text-fg-2 break-words">{item.planName ?? item.id}</p>
        <p className="text-caption text-fg-3">{zh ? "剩余" : "Remaining"}{expired ? zh ? " · 等待刷新" : " · Awaiting refresh" : ""}</p>
        <strong className="block text-2xl tabular-nums break-words" title={`${item.remaining ?? ""} ${item.unit ?? ""}`}>
          {item.remaining != null ? customUsageValue(item.remaining, item.unit, locale) : percent != null ? `${Number(percent.toFixed(2))}%` : "—"}
        </strong>
        {(item.used != null || item.total != null) && <p className="text-caption text-fg-3 break-words">
          {zh ? "已用" : "Used"} {customUsageValue(item.used, item.unit, locale)} · {zh ? "总量" : "Total"} {customUsageValue(item.total, item.unit, locale)}
        </p>}
        {percent != null && <div className="flex items-center gap-2 text-caption tabular-nums">
          <span role="progressbar" aria-label={`${item.planName ?? item.id} ${zh ? "剩余" : "remaining"}`} aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent} className="h-1.5 flex-1 rounded-full bg-subtle overflow-hidden">
            <span className="block h-full bg-action rounded-full" style={{ width: `${percent}%` }} />
          </span><span>{Number(percent.toFixed(2))}%</span>
        </div>}
        {item.resetsAt != null && <p className="text-caption text-fg-3 break-words">
          {zh ? "重置" : "Resets"} {new Date(item.resetsAt * 1000).toLocaleString(locale, { timeZone: timezone, timeZoneName: "short" })}
        </p>}
      </section>;
    })}
  </div>;
}
