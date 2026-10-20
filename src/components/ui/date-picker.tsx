import { useState, type ButtonHTMLAttributes } from "react";
import { useTranslation } from "react-i18next";
import { CalendarDays } from "lucide-react";
import { Button } from "./button";
import { Calendar, formatCalendarDate, parseCalendarDate } from "./calendar";
import { DateInput } from "./date-input";
import { Input, fieldClass } from "./input";
import { Popover, PopoverContent, PopoverTrigger } from "./popover";
import { cn } from "@/lib/utils";

interface DatePickerProps
  extends Omit<
    ButtonHTMLAttributes<HTMLButtonElement>,
    "type" | "value" | "onChange"
  > {
  type?: "date" | "datetime-local";
  value: string;
  onValueChange: (value: string) => void;
  step?: number | string;
  today?: Date;
}

export function DatePicker({
  type = "date",
  value,
  onValueChange,
  step = 60,
  today = new Date(),
  className,
  ...props
}: DatePickerProps) {
  const { t, i18n } = useTranslation();
  const locale = i18n.resolvedLanguage || i18n.language || "en";
  const withTime = type === "datetime-local";
  const [open, setOpen] = useState(false);
  const [date, setDate] = useState("");
  const [time, setTime] = useState("");
  const [month, setMonth] = useState(today);
  const apply = (next: string) => {
    onValueChange(next);
    setOpen(false);
  };

  return (
    <Popover
      open={open}
      onOpenChange={(next) => {
        if (next) {
          const [dateValue, timeValue] = value.split("T");
          const selected = parseCalendarDate(dateValue);
          setDate(selected ? dateValue : "");
          setTime(timeValue || (Number(step) < 60 ? "00:00:00" : "00:00"));
          const day = selected || today;
          setMonth(new Date(day.getFullYear(), day.getMonth(), 1));
        }
        setOpen(next);
      }}
    >
      <PopoverTrigger asChild>
        <button
          {...props}
          type="button"
          className={cn(
            fieldClass,
            "flex h-8 min-w-0 items-center justify-between gap-2 py-1 text-left",
            className,
          )}
        >
          <span className={cn("min-w-0 truncate", !value && "text-fg-3")}>
            {value ? value.replace("T", " ") : "YYYY-MM-DD"}
          </span>
          <CalendarDays className="h-3.5 w-3.5 shrink-0 text-fg-2" />
        </button>
      </PopoverTrigger>
      <PopoverContent className="w-[332px] max-w-[calc(100vw-2rem)] rounded-panel border-border bg-surface p-3 shadow-v7-md">
        <div className="mb-3 flex gap-2">
          <DateInput
            aria-label={props["aria-label"] || t("usage.startTime")}
            className="min-w-0 flex-1"
            value={date}
            onValueChange={(next) => {
              setDate(next);
              const day = parseCalendarDate(next);
              if (day) {
                setMonth(new Date(day.getFullYear(), day.getMonth(), 1));
              }
            }}
            onKeyDown={(event) => {
              if (
                event.key === "Enter" &&
                parseCalendarDate(event.currentTarget.value) &&
                (!withTime || time)
              ) {
                event.preventDefault();
                apply(withTime ? `${date}T${time}` : date);
              }
            }}
          />
          {withTime && (
            <Input
              type="time"
              aria-label={props["aria-label"] || t("usage.startTime")}
              className="w-[132px] shrink-0"
              step={step}
              value={time}
              onChange={(event) => setTime(event.target.value)}
            />
          )}
        </div>
        <Calendar
          locale={locale}
          month={month}
          onMonthChange={setMonth}
          today={today}
          startDate={parseCalendarDate(date)}
          onSelect={(day) => {
            const next = formatCalendarDate(day);
            setDate(next);
            if (!withTime) apply(next);
          }}
        />
        <div className="mt-3 flex gap-2">
          <Button
            type="button"
            variant="quiet"
            size="compact"
            onClick={() => apply("")}
          >
            {t("common.clear")}
          </Button>
          <Button
            type="button"
            variant="quiet"
            size="compact"
            className="ml-auto"
            onClick={() => setOpen(false)}
          >
            {t("common.cancel")}
          </Button>
          <Button
            type="button"
            variant="solid"
            size="compact"
            disabled={!parseCalendarDate(date) || (withTime && !time)}
            onClick={() => apply(withTime ? `${date}T${time}` : date)}
          >
            {t("common.confirm")}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
}
