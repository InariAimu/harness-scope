import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { ChevronLeft, ChevronRight } from "lucide-react";
import { Button } from "./button";
import { HoverTip } from "./hover-tip";
import { cn } from "@/lib/utils";

export function formatCalendarDate(day: Date): string {
  return `${String(day.getFullYear()).padStart(4, "0")}-${String(day.getMonth() + 1).padStart(2, "0")}-${String(day.getDate()).padStart(2, "0")}`;
}

export function parseCalendarDate(value: string): Date | undefined {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return;
  const [year, month, day] = value.split("-").map(Number);
  const date = new Date(0);
  date.setFullYear(year, month - 1, day);
  date.setHours(12, 0, 0, 0);
  if (year < 1 || formatCalendarDate(date) !== value) return;
  return date;
}

interface CalendarProps {
  month: Date;
  onMonthChange: (month: Date) => void;
  onSelect: (day: Date) => void;
  startDate?: Date;
  endDate?: Date;
  today?: Date;
  locale: string;
  className?: string;
}

/** Shared calendar for single dates and the usage date range. */
export function Calendar({
  month,
  onMonthChange,
  onSelect,
  startDate,
  endDate,
  today = new Date(),
  locale,
  className,
}: CalendarProps) {
  const { t } = useTranslation();
  const prevMonthLabel = t("usage.prevMonth", { defaultValue: "上个月" });
  const nextMonthLabel = t("usage.nextMonth", { defaultValue: "下个月" });
  const todayLabel = t("usage.presetToday", { defaultValue: "当天" });
  const days = useMemo(() => {
    const first = new Date(month.getFullYear(), month.getMonth(), 1, 12);
    return Array.from({ length: 42 }, (_, i) =>
      new Date(first.getFullYear(), first.getMonth(), 1 - first.getDay() + i, 12),
    );
  }, [month]);
  const weekdays = useMemo(
    () =>
      Array.from({ length: 7 }, (_, i) =>
        new Intl.DateTimeFormat(locale, { weekday: "narrow" }).format(
          new Date(2024, 0, 7 + i),
        ),
      ),
    [locale],
  );
  const start = startDate && formatCalendarDate(startDate);
  const end = endDate && formatCalendarDate(endDate);
  const current = formatCalendarDate(today);

  return (
    <div
      className={cn(
        "min-w-0 rounded-panel border border-border bg-subtle p-2.5",
        className,
      )}
    >
      <div className="mb-1.5 flex items-center justify-between">
        <HoverTip content={prevMonthLabel}>
          <Button
            type="button"
            size="icon-compact"
            variant="quiet"
            aria-label={prevMonthLabel}
            onClick={() =>
              onMonthChange(new Date(month.getFullYear(), month.getMonth() - 1, 1))
            }
          >
            <ChevronLeft className="h-3.5 w-3.5" />
          </Button>
        </HoverTip>
        <HoverTip content={todayLabel}>
          <button
            type="button"
            className="text-body font-medium text-fg-1 transition-colors hover:text-action-text"
            onClick={() =>
              onMonthChange(new Date(today.getFullYear(), today.getMonth(), 1))
            }
          >
            {month.toLocaleDateString(locale, {
              year: "numeric",
              month: "long",
            })}
          </button>
        </HoverTip>
        <HoverTip content={nextMonthLabel}>
          <Button
            type="button"
            size="icon-compact"
            variant="quiet"
            aria-label={nextMonthLabel}
            onClick={() =>
              onMonthChange(new Date(month.getFullYear(), month.getMonth() + 1, 1))
            }
          >
            <ChevronRight className="h-3.5 w-3.5" />
          </Button>
        </HoverTip>
      </div>
      <div className="mb-0.5 grid grid-cols-7 text-center text-badge text-fg-2">
        {weekdays.map((label, i) => (
          <div key={i} className="py-0.5">
            {label}
          </div>
        ))}
      </div>
      <div className="grid grid-cols-7 gap-px">
        {days.map((day) => {
          const key = formatCalendarDate(day);
          const inMonth =
            day.getMonth() === month.getMonth() &&
            day.getFullYear() === month.getFullYear();
          const isToday = key === current;
          const isEndpoint = key === start || key === end;
          const inRange = !!start && !!end && key >= start && key <= end;
          return (
            <button
              key={key}
              type="button"
              aria-label={day.toLocaleDateString(locale)}
              aria-current={isToday ? "date" : undefined}
              aria-pressed={isEndpoint}
              className={cn(
                "relative h-7 rounded text-caption transition-colors",
                !inMonth && "text-fg-3",
                !isEndpoint && !inRange && "hover:bg-action-soft hover:text-action-text",
                inRange && !isEndpoint && "bg-action-soft text-action-text",
                isEndpoint && "bg-action text-action-fg font-medium",
                isToday && !isEndpoint && "ring-1 ring-border-strong",
              )}
              onClick={() => {
                onSelect(day);
                if (!inMonth) {
                  onMonthChange(new Date(day.getFullYear(), day.getMonth(), 1));
                }
              }}
            >
              {day.getDate()}
            </button>
          );
        })}
      </div>
    </div>
  );
}
