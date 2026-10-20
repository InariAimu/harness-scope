import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { CalendarDays, ChevronDown } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Calendar, parseCalendarDate } from "@/components/ui/calendar";
import { DateInput } from "@/components/ui/date-input";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { cn } from "@/lib/utils";
import {
  getUsageRangePresetLabel,
  resolveUsageRange,
  usageCalendarDate,
  usageDateParts,
  usageTimestamp,
} from "@/lib/usageRange";
import { useUsageTimezone } from "@/lib/query/usageTimezone";
import { getLocaleFromLanguage } from "./format";
import type { UsageRangePreset, UsageRangeSelection } from "@/types/usage";

type DraftField = "start" | "end";

const PRESETS: UsageRangePreset[] = ["today", "1d", "7d", "14d", "30d", "all"];

interface UsageDateRangePickerProps {
  selection: UsageRangeSelection;
  onApply: (selection: UsageRangeSelection) => void;
  triggerLabel: string;
}

/* ── helpers ── */

function fromTs(ts: number, timeZone: string): Date {
  return usageCalendarDate(ts, timeZone);
}

function fmtDate(ts: number, timeZone: string): string {
  const d = fromTs(ts, timeZone);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

function fmtTime(ts: number, timeZone: string): string {
  const p = usageDateParts(ts, timeZone);
  return `${String(p.hour).padStart(2, "0")}:${String(p.minute).padStart(2, "0")}`;
}

function parseDateInput(ts: number, value: string, timeZone: string): number {
  if (!parseCalendarDate(value)) return ts;
  const [y, m, d] = value.split("-").map(Number);
  return usageTimestamp(
    { ...usageDateParts(ts, timeZone), year: y, month: m, day: d, second: 0 },
    timeZone,
  );
}

function parseTimeInput(ts: number, value: string, timeZone: string): number {
  const [h, min] = value.split(":").map(Number);
  if (!Number.isFinite(h) || !Number.isFinite(min)) return ts;
  return usageTimestamp(
    { ...usageDateParts(ts, timeZone), hour: h, minute: min, second: 0 },
    timeZone,
  );
}

function setDateKeepTime(ts: number, day: Date, timeZone: string): number {
  return usageTimestamp(
    {
      ...usageDateParts(ts, timeZone),
      year: day.getFullYear(),
      month: day.getMonth() + 1,
      day: day.getDate(),
      second: 0,
    },
    timeZone,
  );
}

/* ── component ── */

function resolveCalendarRange(
  selection: UsageRangeSelection,
  timeZone: string,
) {
  const r = resolveUsageRange(selection, Date.now(), timeZone);
  if (selection.preset !== "all") return r;
  return { startDate: r.endDate - 30 * 24 * 60 * 60, endDate: r.endDate };
}

export function UsageDateRangePicker({
  selection,
  onApply,
  triggerLabel,
}: UsageDateRangePickerProps) {
  const { t, i18n } = useTranslation();
  const timeZone = useUsageTimezone();
  const [open, setOpen] = useState(false);
  const [activeField, setActiveField] = useState<DraftField>("start");
  // 「全部」的起点是 0，日历草稿改从结束日往前 30 天开始，避免跳到 1970 年
  const resolvedRange = useMemo(
    () => resolveCalendarRange(selection, timeZone),
    [selection, timeZone],
  );
  const [draftStart, setDraftStart] = useState(resolvedRange.startDate);
  const [draftEnd, setDraftEnd] = useState(resolvedRange.endDate);
  const [draftLiveEnd, setDraftLiveEnd] = useState(
    selection.preset === "custom" ? (selection.liveEndTime ?? false) : false,
  );
  const [displayMonth, setDisplayMonth] = useState(
    () =>
      new Date(
        fromTs(resolvedRange.startDate, timeZone).getFullYear(),
        fromTs(resolvedRange.startDate, timeZone).getMonth(),
        1,
      ),
  );
  const [error, setError] = useState<string | null>(null);

  const language = i18n.resolvedLanguage || i18n.language || "en";
  const locale = getLocaleFromLanguage(language);

  // Reset draft when popover opens
  useEffect(() => {
    if (!open) return;
    const r = resolveCalendarRange(selection, timeZone);
    setDraftStart(r.startDate);
    setDraftEnd(r.endDate);
    setDraftLiveEnd(
      selection.preset === "custom" ? (selection.liveEndTime ?? false) : false,
    );
    setDisplayMonth(
      new Date(
        fromTs(r.startDate, timeZone).getFullYear(),
        fromTs(r.startDate, timeZone).getMonth(),
        1,
      ),
    );
    setActiveField("start");
    setError(null);
  }, [open, selection, timeZone]);

  // Keep draftEnd ticking when live mode is active and popover is open
  useEffect(() => {
    if (!open || !draftLiveEnd) return;
    const tick = () => setDraftEnd(Math.floor(Date.now() / 1000));
    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, [open, draftLiveEnd]);

  const startDay = fromTs(draftStart, timeZone);
  const endDay = fromTs(draftEnd, timeZone);
  const today = fromTs(Date.now() / 1000, timeZone);

  /* Pick a date from the calendar */
  const handleDatePick = (day: Date) => {
    setError(null);

    // When live end time is active, calendar only controls start date
    if (draftLiveEnd) {
      const nextTs = setDateKeepTime(draftStart, day, timeZone);
      setDraftStart(nextTs);
      return;
    }

    const nextTs = setDateKeepTime(
      activeField === "start" ? draftStart : draftEnd,
      day,
      timeZone,
    );

    if (activeField === "start") {
      setDraftStart(nextTs);
      // Auto-swap if start > end
      if (nextTs > draftEnd) {
        setDraftEnd(nextTs);
      }
      // Auto-advance to end field
      setActiveField("end");
    } else {
      // If picked end < start, treat as new start and auto-advance
      if (nextTs < draftStart) {
        setDraftStart(nextTs);
        setActiveField("end");
      } else {
        setDraftEnd(nextTs);
      }
    }
  };

  const handleApply = () => {
    setError(null);
    if (draftStart > draftEnd) {
      setError(t("usage.invalidTimeRangeOrder", "开始时间不能晚于结束时间"));
      return;
    }
    onApply({
      preset: "custom",
      customStartDate: draftStart,
      customEndDate: draftEnd,
      liveEndTime: draftLiveEnd,
    });
    setOpen(false);
  };

  /* ── Field card (start / end) ── */
  const renderField = (field: DraftField) => {
    const isActive = activeField === field;
    const isEndLive = field === "end" && draftLiveEnd;
    const ts = field === "start" ? draftStart : draftEnd;
    const setTs = field === "start" ? setDraftStart : setDraftEnd;
    const label =
      field === "start"
        ? t("usage.startTime", "开始时间")
        : t("usage.endTime", "结束时间");

    return (
      <div
        className={cn(
          "rounded-panel border px-3 py-2 transition-all",
          isEndLive
            ? "border-border bg-subtle cursor-not-allowed opacity-50"
            : isActive
              ? "border-ring ring-1 ring-ring bg-surface cursor-pointer"
              : "border-border hover:border-border cursor-pointer",
        )}
        onClick={() => {
          if (!isEndLive) setActiveField(field);
        }}
      >
        <div className="mb-1.5 text-badge font-medium uppercase tracking-wider text-fg-2">
          {label}
        </div>
        <div className="flex items-center gap-1.5">
          <DateInput
            aria-label={label}
            className={cn(
              "h-7 flex-1 border-0 bg-transparent p-0 text-body shadow-none focus:ring-0 focus-visible:ring-0",
              isEndLive && "pointer-events-none",
            )}
            value={fmtDate(ts, timeZone)}
            onValueChange={(value) => {
              if (isEndLive) return;
              const next = parseDateInput(ts, value, timeZone);
              setTs(next);
              const d = fromTs(next, timeZone);
              setDisplayMonth(new Date(d.getFullYear(), d.getMonth(), 1));
              setError(null);
            }}
            onFocus={() => {
              if (!isEndLive) setActiveField(field);
            }}
            readOnly={isEndLive}
          />
          <Input
            type="time"
            aria-label={label}
            step={60}
            className={cn(
              "h-7 w-[90px] flex-none border-0 bg-transparent p-0 text-body shadow-none focus:ring-0 focus-visible:ring-0",
              isEndLive && "pointer-events-none",
            )}
            value={fmtTime(ts, timeZone)}
            onChange={(e) => {
              if (isEndLive) return;
              setTs(parseTimeInput(ts, e.target.value, timeZone));
              setError(null);
            }}
            onFocus={() => {
              if (!isEndLive) setActiveField(field);
            }}
            readOnly={isEndLive}
          />
        </div>
      </div>
    );
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          type="button"
          variant="neutral"
          size="regular"
          className="max-w-[220px] shrink-0 gap-1 pe-2 ps-3"
          title={`${t("usage.timeRange")}: ${triggerLabel}`}
        >
          {selection.preset === "custom" && (
            <CalendarDays className="h-3.5 w-3.5 shrink-0 text-fg-2" />
          )}
          <span className="min-w-0 truncate">{triggerLabel}</span>
          <ChevronDown className="h-3.5 w-3.5 shrink-0 text-fg-2" />
        </Button>
      </PopoverTrigger>
      <PopoverContent
        className="usage-range-popover w-[620px] max-w-[calc(100vw-2rem)] rounded-panel border-border bg-surface p-3 shadow-v7-md"
        align="end"
      >
        {/* Preset shortcuts */}
        <div className="flex flex-wrap gap-1.5 pb-2 border-b border-border">
          {PRESETS.map((preset) => (
            <Button
              key={preset}
              type="button"
              size="compact"
              variant="neutral"
              aria-pressed={selection.preset === preset}
              className={cn(
                "px-2.5",
                selection.preset === preset &&
                  "border-ring bg-action-soft font-semibold text-action-text",
              )}
              onClick={() => {
                onApply({ preset });
                setOpen(false);
              }}
            >
              {getUsageRangePresetLabel(preset, t)}
            </Button>
          ))}
        </div>

        <div className="usage-range-layout mt-3 flex flex-col gap-3">
          {/* Left: date fields */}
          <div className="usage-range-fields space-y-2">
            <p className="text-caption text-fg-2">
              {t("usage.customRangeHint", "支持日期与时间，最长 30 天")}
            </p>
            {renderField("start")}
            {renderField("end")}

            <label className="flex items-center gap-2 cursor-pointer select-none">
              <Checkbox
                checked={draftLiveEnd}
                onCheckedChange={(checked) => {
                  const live = checked === true;
                  setDraftLiveEnd(live);
                  if (live) {
                    setDraftEnd(Math.floor(Date.now() / 1000));
                    setActiveField("start");
                  }
                }}
              />
              <span className="text-caption text-fg-2">
                {t("usage.liveEndTime", "结束时间跟随当前时刻")}
              </span>
            </label>

            {error && <p className="text-caption text-danger-text">{error}</p>}

            <div className="flex gap-2 pt-1">
              <Button
                type="button"
                variant="quiet"
                size="compact"
                className="flex-1"
                onClick={() => setOpen(false)}
              >
                {t("common.cancel")}
              </Button>
              <Button
                type="button"
                variant="solid"
                size="compact"
                className="flex-1"
                onClick={handleApply}
              >
                {t("common.confirm")}
              </Button>
            </div>
          </div>

          {/* Right: calendar */}
          <Calendar
            className="usage-range-calendar"
            locale={locale}
            month={displayMonth}
            onMonthChange={setDisplayMonth}
            startDate={startDay}
            endDate={endDay}
            today={today}
            onSelect={handleDatePick}
          />
        </div>
      </PopoverContent>
    </Popover>
  );
}
