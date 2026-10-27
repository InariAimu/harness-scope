import { Button } from "@/components/ui/button";
import { DatePicker } from "@/components/ui/date-picker";
import { ProviderHelp } from "./ProviderHelp";
import { usageCalendarDate } from "@/lib/usageRange";
import {
  deadlineDraft,
  deadlineWallTime,
  parseDeadlineWallTime,
  type DeadlineDraft,
} from "@/lib/codexDeadline";

export function CodexDeadlineInput({
  draft,
  onChange,
  timeZone,
  officialReset,
  zh,
}: {
  draft: DeadlineDraft;
  onChange: (value: DeadlineDraft) => void;
  timeZone: string;
  officialReset: number | null;
  zh: boolean;
}) {
  const l = (a: string, b: string) => (zh ? a : b);
  const setNow = () =>
    onChange(deadlineDraft(Math.floor(Date.now() / 1000), timeZone));
  const edit = (value: string, pst: boolean) => {
    const at = parseDeadlineWallTime(value, pst ? "PST" : timeZone);
    onChange({
      at,
      local: pst ? (at == null ? "" : deadlineWallTime(at, timeZone)) : value,
      pst: pst ? value : at == null ? "" : deadlineWallTime(at, "PST"),
    });
  };
  return (
    <div className="space-y-3">
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <label className="flex flex-col gap-1 text-caption text-fg-2">
          {l("目标时间（当前时区）", "Target time (current time zone)")} ·{" "}
          {timeZone}
          <DatePicker
            type="datetime-local"
            step="1"
            aria-label={l(
              "目标时间（当前时区）",
              "Target time (current time zone)",
            )}
            value={draft.local}
            onValueChange={(value) => edit(value, false)}
            onNow={setNow}
            today={usageCalendarDate(Date.now() / 1000, timeZone)}
            aria-invalid={draft.at == null}
          />
        </label>
        <label className="flex flex-col gap-1 text-caption text-fg-2">
          {l("目标时间（PST）", "Target time (PST)")} · UTC−08:00
          <DatePicker
            type="datetime-local"
            step="1"
            aria-label={l("目标时间（PST）", "Target time (PST)")}
            value={draft.pst}
            onValueChange={(value) => edit(value, true)}
            onNow={setNow}
            today={usageCalendarDate(Date.now() / 1000 - 8 * 3600, "UTC")}
            aria-invalid={draft.at == null}
          />
        </label>
      </div>
      <div className="flex flex-wrap items-center gap-2 text-caption text-fg-3">
        <Button
          variant="quiet"
          size="compact"
          disabled={officialReset == null}
          onClick={() =>
            officialReset != null &&
            onChange(deadlineDraft(officialReset, timeZone))
          }
        >
          {l("使用官方周重置时间", "Use official weekly reset")}
        </Button>
        <ProviderHelp label={l("目标时间说明", "About target time")}>
          {l(
            "PST 固定 UTC−8，不随夏令时切换 PDT。两个时间框表示同一时刻。以当前每周剩余额度为目标，按各模型本次窗口的样本估算均匀使用强度。不会改写官方重置时间，也不计入下一窗口的补充额度。",
            "PST is fixed UTC−8, without PDT. Both fields represent the same instant. Plans the current weekly remainder using each model's current-window samples. Official resets are kept; future replenishments are excluded.",
          )}
        </ProviderHelp>
      </div>
    </div>
  );
}
