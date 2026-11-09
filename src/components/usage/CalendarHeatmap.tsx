import { Fragment, useId, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { cn } from "@/lib/utils";
import { getLocaleFromLanguage } from "./format";
import { monthLabelVisibility } from "./heatmapMonths";
import { buildCalendarHeatmap, calendarDayKey, calendarLevels, type CalendarCell, type CalendarDatum } from "./heatmapCalendar";

function cellStyle(level: number) { return level ? { backgroundColor: `var(--heat-${level})` } : undefined; }

function HeatmapMonths({ months, locale }: { months: (number | null)[]; locale: string }) {
  const anchor = useRef<HTMLSpanElement>(null);
  const [visible, setVisible] = useState<Set<number> | null>(null);
  useLayoutEffect(() => {
    const grid = anchor.current?.parentElement;
    if (!grid) return;
    let active = true;
    const measure = () => {
      if (!active) return;
      const labels = [...grid.querySelectorAll<HTMLElement>("[data-heatmap-month]")];
      const visibility = monthLabelVisibility(labels.map(label => label.getBoundingClientRect()));
      const next = new Set(labels.filter((_, i) => visibility[i]).map(label => Number(label.dataset.heatmapMonth)));
      setVisible(previous => previous?.size === next.size && [...next].every(i => previous.has(i)) ? previous : next);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(grid);
    void document.fonts.ready.then(measure);
    return () => { active = false; observer.disconnect(); };
  }, [months, locale]);
  return <><span ref={anchor} />{months.map((month, week) => (
    <span key={week} className="h-4 overflow-visible whitespace-nowrap text-badge leading-4 text-fg-3">
      {month != null && <span data-heatmap-month={week} style={{ visibility: visible == null || visible.has(week) ? "inherit" : "hidden" }}>
        {new Date(2024, month - 1, 1).toLocaleDateString(locale, { month: "short" })}
      </span>}
    </span>
  ))}</>;
}

interface CalendarHeatmapProps<T extends CalendarDatum> {
  entries: T[];
  today: Date;
  spanCount?: number;
  valueOf: (datum: T) => number | null;
  lowerIsDarker?: boolean;
  title: string;
  help: string;
  isLoading?: boolean;
  controls?: ReactNode;
  composition?: ReactNode;
  renderSummary: (entries: T[]) => ReactNode;
  renderTooltip: (date: string, datum?: T) => ReactNode;
  legendLow?: string;
  legendHigh?: string;
  testId?: string;
}

/** Shared calendar, month layout, history expansion, color scale and hover placement. */
export function CalendarHeatmap<T extends CalendarDatum>({ entries, today, spanCount, valueOf, lowerIsDarker, title, help,
  isLoading, controls, composition, renderSummary, renderTooltip, legendLow, legendHigh, testId }: CalendarHeatmapProps<T>) {
  const { t, i18n } = useTranslation();
  const locale = getLocaleFromLanguage(i18n.resolvedLanguage || i18n.language || "en");
  const id = useId();
  const sectionRef = useRef<HTMLElement>(null);
  const [showEarlier, setShowEarlier] = useState(false);
  const [hover, setHover] = useState<{ cell: CalendarCell<T>; x: number; y: number } | null>(null);
  const todayKey = calendarDayKey(today);
  const spans = useMemo(() => buildCalendarHeatmap(entries, valueOf, today, spanCount), [entries, valueOf, todayKey, spanCount]);
  const levelOf = useMemo(() => calendarLevels(spans.flatMap(span => span.weeks.flat()).filter(cell => !cell.future).map(cell => cell.value), lowerIsDarker), [spans, lowerIsDarker]);
  const visibleSpans = showEarlier ? spans : spans.slice(0, 1);
  const formatDay = (date: Date) => date.toLocaleDateString(locale, { year: "numeric", month: "short", day: "numeric" });
  const weekdayLabels = useMemo(() => Array.from({ length: 7 }, (_, day) => new Date(2024, 0, 1 + day).toLocaleDateString(locale, { weekday: "short" })), [locale]);
  const showTooltip = (cell: CalendarCell<T>, target: HTMLElement) => {
    const box = sectionRef.current?.getBoundingClientRect();
    if (!box) return;
    const rect = target.getBoundingClientRect();
    setHover({ cell, x: rect.left + rect.width / 2 - box.left, y: rect.top - box.top });
  };
  // Resolve the hovered date against current data after a background refresh/filter change.
  const hoveredDatum = hover ? entries.find(entry => entry.date === hover.cell.key) : undefined;
  return (
    <section ref={sectionRef} aria-labelledby={id} data-testid={testId} className="relative shrink-0 rounded-panel border border-border bg-surface px-4 py-3">
      <div className="flex min-h-7 flex-wrap items-center gap-x-3.5 gap-y-1">
        <div className="flex items-center gap-1">
          <h2 id={id} className="m-0 whitespace-nowrap text-body font-semibold text-fg-1">{title}</h2>
          {!composition && <HelpTip icon="info" title={title}>{help}</HelpTip>}
        </div>{controls}
      </div>
      <div className="relative">
        <div className={composition ? "invisible" : undefined}>
          {isLoading ? <div className="flex h-[150px] items-center justify-center"><Loader2 className="h-5 w-5 animate-spin text-fg-3" /></div> : visibleSpans.map((span, index) => (
            <div key={calendarDayKey(span.start)} className={index === 0 ? "mt-3" : "mt-4"} data-testid="heatmap-span">
              <div className="flex flex-wrap items-baseline gap-x-2">
                <h3 className="m-0 text-body font-semibold tabular-nums text-fg-1">{`${formatDay(span.start)} – ${formatDay(span.end)}`}</h3>
                <span className="text-caption tabular-nums text-fg-3">{renderSummary(span.entries)}</span>
              </div>
              <div className="mt-1.5 grid items-center gap-[3px]" style={{ gridTemplateColumns: "max-content repeat(53, minmax(0, 1fr))" }}>
                <HeatmapMonths months={span.monthLabels} locale={locale} />
                {weekdayLabels.map((label, day) => <Fragment key={day}>
                  <span className="pe-1.5 text-badge leading-none text-fg-3">{day % 2 === 0 ? label : ""}</span>
                  {span.weeks.map(column => {
                    const cell = column[day], level = levelOf(cell.value);
                    return <div key={cell.key} data-date={cell.key} data-level={cell.future ? undefined : level}
                      onMouseEnter={cell.future ? undefined : event => showTooltip(cell, event.currentTarget)} onMouseLeave={() => setHover(null)}
                      className={cn("aspect-square w-full rounded-[3px]", !cell.future && level === 0 && "bg-subtle")}
                      style={cell.future ? undefined : cellStyle(level)} />;
                  })}
                </Fragment>)}
              </div>
            </div>
          ))}
        </div>
        {composition && <div className="absolute inset-0 pt-3">{composition}</div>}
      </div>
      {hover && !composition && <div role="tooltip" className="pointer-events-none absolute z-20" style={{ left: hover.x, top: hover.y - 8,
        transform: `translate(${hover.x < 110 ? "-15%" : hover.x > (sectionRef.current?.clientWidth ?? 0) - 110 ? "-85%" : "-50%"}, -100%)` }}>
        {renderTooltip(hover.cell.date.toLocaleDateString(locale, { year: "numeric", month: "long", day: "numeric", weekday: "short" }), hoveredDatum)}
      </div>}
      <div className={cn("mt-2 flex items-center gap-1.5 text-badge text-fg-3", composition && "invisible")}>
        {!isLoading && spans.length > 1 && <Button type="button" variant="quiet" size="compact" aria-expanded={showEarlier}
          onClick={() => setShowEarlier(open => !open)} className="-ms-2 gap-1 pe-2 ps-2 text-caption text-fg-2">
          {showEarlier ? t("usage.heatmap.showLess") : t("usage.heatmap.showMore")}
          <ChevronDown className={cn("h-3.5 w-3.5 transition-transform duration-150", showEarlier && "rotate-180")} />
        </Button>}
        <div className="flex-1" />{legendLow ?? t("usage.heatmap.less")}
        {[0, 1, 2, 3, 4].map(level => <span key={level} className={cn("h-2.5 w-2.5 rounded-[2px]", level === 0 && "bg-subtle")} style={cellStyle(level)} />)}
        {legendHigh ?? t("usage.heatmap.more")}
      </div>
    </section>
  );
}
