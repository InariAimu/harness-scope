const WEEKS = 53;
const DAY_MS = 86_400_000;
export const calendarDayKey = (date: Date) =>
  `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
export const calendarStartOfDay = (date: Date) => new Date(date.getFullYear(), date.getMonth(), date.getDate());
const addDays = (date: Date, days: number) => new Date(date.getFullYear(), date.getMonth(), date.getDate() + days);
const weekdayIndex = (date: Date) => (date.getDay() + 6) % 7;
export function calendarSpanStart(today: Date, index: number) {
  return addDays(today, -weekdayIndex(today) - ((WEEKS - 1) * 7 + index * WEEKS * 7));
}
export function calendarSpanCount(firstDate: string | null | undefined, today: Date) {
  const match = firstDate ? /^(\d{4})-(\d{2})-(\d{2})$/.exec(firstDate) : null;
  if (!match) return 1;
  const first = new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3]));
  const latestStart = calendarSpanStart(today, 0);
  const before = Math.round((Date.UTC(latestStart.getFullYear(), latestStart.getMonth(), latestStart.getDate()) -
    Date.UTC(first.getFullYear(), first.getMonth(), first.getDate())) / DAY_MS);
  return before > 0 ? 1 + Math.ceil(before / (WEEKS * 7)) : 1;
}
export interface CalendarDatum { date: string }
export interface CalendarCell<T> {
  key: string;
  date: Date;
  datum?: T;
  value: number | null;
  future: boolean;
}
export interface CalendarSpan<T> {
  start: Date;
  end: Date;
  weeks: CalendarCell<T>[][];
  monthLabels: (number | null)[];
  entries: T[];
}
/** Dates are civil dates, not UTC timestamps. Empty days remain null, including valid zero TTFT. */
export function buildCalendarHeatmap<T extends CalendarDatum>(entries: T[], valueOf: (datum: T) => number | null, today: Date, count?: number) {
  const byDay = new Map(entries.map(entry => [entry.date, entry]));
  const first = entries.reduce<string | undefined>((date, entry) => date == null || entry.date < date ? entry.date : date, undefined);
  const spans: CalendarSpan<T>[] = Array.from({ length: count ?? calendarSpanCount(first, today) }, (_, index) => {
    const start = calendarSpanStart(today, index);
    const end = index === 0 ? today : addDays(calendarSpanStart(today, index - 1), -1);
    const weeks = Array.from({ length: WEEKS }, (_, week) => Array.from({ length: 7 }, (_, day) => {
      const date = addDays(start, week * 7 + day);
      const key = calendarDayKey(date), datum = byDay.get(key);
      const value = datum ? valueOf(datum) : null;
      return { key, date, datum, value: value != null && Number.isFinite(value) && value >= 0 ? value : null, future: date > today };
    }));
    const monthLabels = weeks.map((column, week) => {
      const first = column.find(cell => cell.date.getDate() === 1 && !cell.future);
      return first ? first.date.getMonth() + 1 : week === 0 ? column[0].date.getMonth() + 1 : null;
    });
    return { start, end, weeks, monthLabels, entries: weeks.flat().flatMap(cell => !cell.future && cell.datum ? [cell.datum] : []) };
  });
  return spans;
}
/** One scale across all spans. Reversing the score makes lower latency darker, including zero. */
export function calendarLevels(values: (number | null)[], lowerIsDarker = false) {
  const score = (value: number) => lowerIsDarker ? -value : value;
  const sorted = values.filter((value): value is number => value != null && Number.isFinite(value)).map(score).sort((a, b) => a - b);
  const at = (q: number) => sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * q))] ?? Infinity;
  const thresholds = [at(.25), at(.5), at(.75)];
  return (value: number | null) => value == null ? 0 : score(value) <= thresholds[0] ? 1 : score(value) <= thresholds[1] ? 2 : score(value) <= thresholds[2] ? 3 : 4;
}
