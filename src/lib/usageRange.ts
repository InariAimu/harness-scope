import type { UsageRangePreset, UsageRangeSelection } from "@/types/usage";

const DAY_SECONDS = 24 * 60 * 60;

export interface UsageDateParts {
  year: number;
  month: number;
  day: number;
  hour?: number;
  minute?: number;
  second?: number;
}
const formatters = new Map<string, Intl.DateTimeFormat>();
export function usageDateParts(
  timestamp: number,
  timeZone?: string,
): Required<UsageDateParts> {
  const key = timeZone ?? Intl.DateTimeFormat().resolvedOptions().timeZone;
  let formatter = formatters.get(key);
  if (!formatter) {
    formatter = new Intl.DateTimeFormat("en-US-u-ca-gregory-nu-latn", {
      timeZone: key,
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hourCycle: "h23",
    });
    formatters.set(key, formatter);
  }
  const parts = Object.fromEntries(
    formatter
      .formatToParts(timestamp * 1000)
      .map((p) => [p.type, Number(p.value)]),
  );
  return {
    year: parts.year,
    month: parts.month,
    day: parts.day,
    hour: parts.hour,
    minute: parts.minute,
    second: parts.second,
  };
}

/** Convert wall time to an instant. Gaps advance; overlaps choose the earlier instant. */
export function usageTimestamp(
  parts: UsageDateParts,
  timeZone?: string,
): number {
  const utc = (p: UsageDateParts) =>
    Date.UTC(
      p.year,
      p.month - 1,
      p.day,
      p.hour ?? 0,
      p.minute ?? 0,
      p.second ?? 0,
    ) / 1000;
  const target = utc(parts);
  const candidates = [-DAY_SECONDS, DAY_SECONDS].map((delta) => {
    const sample = target + delta;
    return target - (utc(usageDateParts(sample, timeZone)) - sample);
  });
  const matches = candidates.filter(
    (ts) => utc(usageDateParts(ts, timeZone)) === target,
  );
  return matches.length ? Math.min(...matches) : Math.max(...candidates);
}

/** A calendar-only Date; noon avoids the browser zone's midnight DST transitions. */
export function usageCalendarDate(timestamp: number, timeZone?: string): Date {
  const p = usageDateParts(timestamp, timeZone);
  return new Date(p.year, p.month - 1, p.day, 12);
}

export interface ResolvedUsageRange {
  startDate: number;
  endDate: number;
}

function getPresetLookbackStart(
  preset: "today" | "7d" | "14d" | "30d",
  nowMs: number,
  timeZone?: string,
): number {
  const dayCount =
    preset === "today" ? 1 : preset === "7d" ? 7 : preset === "14d" ? 14 : 30;
  const p = usageDateParts(Math.floor(nowMs / 1000), timeZone);
  const day = new Date(Date.UTC(p.year, p.month - 1, p.day - dayCount + 1));
  return usageTimestamp(
    {
      year: day.getUTCFullYear(),
      month: day.getUTCMonth() + 1,
      day: day.getUTCDate(),
    },
    timeZone,
  );
}

export function resolveUsageRange(
  selection: UsageRangeSelection,
  nowMs: number = Date.now(),
  timeZone?: string,
): ResolvedUsageRange {
  const endDate = Math.floor(nowMs / 1000);

  switch (selection.preset) {
    case "today":
      return {
        startDate: getPresetLookbackStart("today", nowMs, timeZone),
        endDate,
      };
    case "1d":
      return {
        startDate: endDate - DAY_SECONDS,
        endDate,
      };
    case "7d":
    case "14d":
    case "30d":
      return {
        startDate: getPresetLookbackStart(selection.preset, nowMs, timeZone),
        endDate,
      };
    // 全部：从最早的记录算起，汇总和明细表按全量统计
    case "all":
      return { startDate: 0, endDate };
    case "custom": {
      const startDate = selection.customStartDate ?? endDate - DAY_SECONDS;
      const customEndDate = selection.liveEndTime
        ? endDate
        : (selection.customEndDate ?? endDate);
      return {
        startDate,
        endDate: customEndDate,
      };
    }
  }
}

export function getUsageRangePresetLabel(
  preset: UsageRangePreset,
  t: (key: string, options?: { defaultValue?: string }) => string,
): string {
  switch (preset) {
    case "today":
      return t("usage.presetToday", { defaultValue: "当天" });
    case "1d":
      return t("usage.preset1d", { defaultValue: "1d" });
    case "7d":
      return t("usage.preset7d", { defaultValue: "7d" });
    case "14d":
      return t("usage.preset14d", { defaultValue: "14d" });
    case "30d":
      return t("usage.preset30d", { defaultValue: "30d" });
    case "all":
      return t("usage.presetAll", { defaultValue: "全部" });
    case "custom":
      return t("usage.customRange", { defaultValue: "日历筛选" });
  }
}
