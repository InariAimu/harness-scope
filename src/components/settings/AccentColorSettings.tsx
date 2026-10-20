import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Check } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { OptionSelect } from "@/components/ui/option-select";
import { DatePicker } from "@/components/ui/date-picker";
import { formatCalendarDate } from "@/components/ui/calendar";
import { cn } from "@/lib/utils";
import { COMMUNITY_ACCENT_PRESETS } from "@/lib/communityAccentPresets";
import {
  DEFAULT_ACCENT_COLOR,
  createAccentPalette,
  normalizeAccentColor,
} from "@/lib/accentColor";

const PRESETS = [
  ["#f97316", "橙色", "Orange"],
  ["#2563eb", "蓝色", "Blue"],
  ["#7c3aed", "紫色", "Purple"],
  ["#059669", "绿色", "Green"],
  ["#e11d48", "玫红", "Rose"],
  ["#52525b", "石墨", "Graphite"],
];

export function AccentColorSettings({
  value,
  onChange,
  onValidityChange,
}: {
  value: string;
  onChange: (color: string) => void;
  onValidityChange: (valid: boolean) => void;
}) {
  const { i18n } = useTranslation();
  const l = (zh: string, en: string) =>
    i18n.language.startsWith("zh") ? zh : en;
  const [text, setText] = useState(value);
  const [previewOption, setPreviewOption] = useState("first");
  const [previewDate, setPreviewDate] = useState(() =>
    formatCalendarDate(new Date()),
  );
  const valid = normalizeAccentColor(text) !== null;
  const palette = createAccentPalette(value);
  useEffect(() => {
    // Keep a valid shorthand while typing, so #RGB does not interrupt #RRGGBB entry.
    setText((current) =>
      normalizeAccentColor(current) === value ? current : value,
    );
  }, [value]);
  useEffect(() => onValidityChange(valid), [valid, onValidityChange]);
  useEffect(() => () => onValidityChange(true), [onValidityChange]);
  const choose = (color: string) => {
    setText(color);
    onChange(color);
  };

  return (
    <fieldset className="mb-5 min-w-0 space-y-3">
      <legend className="mb-2 text-caption text-fg-2">
        {l("重点色", "Accent color")}
      </legend>
      <div className="flex flex-wrap items-center gap-2">
        <Input
          type="color"
          aria-label={l("选择重点色", "Choose accent color")}
          className="w-10 shrink-0 cursor-pointer p-1"
          value={palette.base}
          onChange={(event) => choose(event.target.value)}
        />
        <Input
          aria-label={l("重点色色值", "Accent HEX value")}
          className="w-32 font-mono"
          value={text}
          maxLength={7}
          aria-invalid={!valid}
          onChange={(event) => {
            const next = event.target.value;
            setText(next);
            const color = normalizeAccentColor(next);
            if (color) onChange(color);
          }}
          onBlur={() => {
            if (valid) setText(palette.base);
          }}
        />
        <Button
          type="button"
          variant="neutral"
          onClick={() => choose(DEFAULT_ACCENT_COLOR)}
        >
          {l("恢复默认", "Restore default")}
        </Button>
      </div>
      {!valid && (
        <p role="alert" className="text-caption text-danger-text">
          {l(
            "请输入有效的 HEX 颜色，例如 #2563EB。",
            "Enter a valid HEX color, such as #2563EB.",
          )}
        </p>
      )}
      <div
        className="flex flex-wrap gap-2"
        aria-label={l("预设重点色", "Accent presets")}
      >
        {PRESETS.map(([color, zh, en]) => (
          <button
            key={color}
            type="button"
            aria-label={l(zh, en)}
            title={l(zh, en)}
            aria-pressed={palette.base === color}
            className={cn(
              "h-7 w-7 rounded-control border border-border-strong focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
              palette.base === color &&
                "ring-2 ring-ring ring-offset-2 ring-offset-background",
            )}
            style={{ backgroundColor: color }}
            onClick={() => choose(color)}
          />
        ))}
      </div>
      <fieldset className="min-w-0 space-y-2">
        <legend className="mb-1 text-caption text-fg-2">
          {l("社区 AI 配色", "Community AI palettes")}
        </legend>
        <p className="text-caption text-fg-3">
          {l(
            "从社区 AI 拟人形象中提取的配色。",
            "Colors sampled from community AI character artwork.",
          )}
        </p>
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2 xl:grid-cols-3">
          {COMMUNITY_ACCENT_PRESETS.map((preset) => {
            const selected = palette.base === preset.color;
            const name = l(preset.name.zh, preset.name.en);
            return (
              <button
                key={preset.id}
                type="button"
                aria-label={name}
                aria-pressed={selected}
                title={`${name} · ${preset.color.toUpperCase()}`}
                onClick={() => choose(preset.color)}
                className={cn(
                  "flex min-w-0 items-center gap-2 rounded-control border p-2 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
                  selected
                    ? "border-action bg-action-soft"
                    : "border-border-strong hover:bg-subtle",
                )}
              >
                <img
                  src={preset.sourceImage}
                  alt=""
                  width={64}
                  height={36}
                  loading="lazy"
                  className="h-9 w-16 shrink-0 rounded-sm object-contain"
                />
                <span className="min-w-0 flex-1">
                  <span className="block text-caption font-medium">{name}</span>
                  <span className="flex items-center gap-1.5 font-mono text-caption text-fg-2">
                    <span
                      aria-hidden="true"
                      className="h-3 w-3 shrink-0 rounded-full border border-border-strong"
                      style={{ backgroundColor: preset.color }}
                    />
                    {preset.color.toUpperCase()}
                  </span>
                </span>
                <Check
                  aria-hidden="true"
                  size={14}
                  className={cn("shrink-0 text-action-text", !selected && "invisible")}
                />
              </button>
            );
          })}
        </div>
      </fieldset>
      <div className="grid grid-cols-3 gap-2" aria-live="polite">
        {[
          [
            l("−1 · 深色", "−1 · Darker"),
            palette.minus,
            palette.foregroundMinus,
          ],
          [l("0 · 重点色", "0 · Accent"), palette.base, palette.foreground],
          [
            l("+1 · 浅色", "+1 · Lighter"),
            palette.plus,
            palette.foregroundPlus,
          ],
        ].map(([label, color, foreground]) => (
          <div
            key={label}
            className="min-w-0 rounded-control px-2 py-3"
            style={{ backgroundColor: color, color: foreground }}
          >
            <div className="text-caption">{label}</div>
            <div className="font-mono text-caption">{color.toUpperCase()}</div>
          </div>
        ))}
      </div>
      <div className="space-y-2">
        <p className="text-caption text-fg-2">
          {l("实时预览", "Live preview")}
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <Button type="button">{l("按钮预览", "Button preview")}</Button>
          <OptionSelect
            aria-label={l("下拉框预览", "Dropdown preview")}
            value={previewOption}
            onValueChange={setPreviewOption}
          >
            <option value="first">{l("选项一", "Option one")}</option>
            <option value="second">{l("选项二", "Option two")}</option>
          </OptionSelect>
          <DatePicker
            aria-label={l("日期面板预览", "Date picker preview")}
            className="w-36"
            value={previewDate}
            onValueChange={setPreviewDate}
          />
        </div>
        <div
          className="flex h-12 items-end gap-2"
          role="img"
          aria-label={l("图表配色预览", "Chart color preview")}
        >
          {[35, 55, 42, 78, 64, 100, 82].map((height, i) => (
            <span
              key={i}
              className="w-5 rounded-t bg-chart-1"
              style={{ height: `${height}%` }}
            />
          ))}
        </div>
      </div>
      <p className="text-caption text-fg-2">
        {l(
          "即时预览整个界面，保存设置后保留；未保存就离开将恢复原配色。灰阶、成功、警告和错误颜色保持独立。",
          "Preview across the interface now; save to keep it. Leaving without saving restores the saved colors. Neutral and status colors stay independent.",
        )}
      </p>
    </fieldset>
  );
}
