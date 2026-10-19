import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { usageApi } from "@/lib/api/usage";
import { usageKeys } from "@/lib/query/usage";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { toast } from "@/lib/toast";
import type { PricingSettings } from "@/types/usage";

export const pricingSettingsKey = ["pricing-settings"] as const;
export function usePricingSettings() {
  return useQuery({
    queryKey: pricingSettingsKey,
    queryFn: usageApi.getPricingSettings,
  });
}

function SettingsForm({ settings }: { settings: PricingSettings }) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const [currency, setCurrency] = useState("CNY");
  const [rate, setRate] = useState(settings.exchangeRates.CNY ?? "");
  const [year, setYear] = useState(String(new Date().getFullYear()));
  const [dates, setDates] = useState(
    (settings.holidayCalendars[year] ?? []).join("\n"),
  );
  const save = useMutation({
    mutationFn: (patch: Partial<PricingSettings>) =>
      usageApi.savePricingSettings(patch),
    onSuccess: (value) => {
      client.setQueryData(pricingSettingsKey, value);
      client.invalidateQueries({ queryKey: usageKeys.all });
      toast.success(
        t(
          "scope.pricing.settingsSaved",
          "计价设置已保存，已补齐可计价的未知费用",
        ),
      );
    },
    onError: (error) => toast.error(String(error)),
  });
  const reprice = useMutation({
    mutationFn: () => usageApi.repriceUsage(),
    onSuccess: (count) => {
      client.invalidateQueries({ queryKey: usageKeys.all });
      toast.success(
        t("scope.pricing.repriced", {
          defaultValue: "已重算 {{count}} 条本机估算费用",
          count,
        }),
      );
    },
    onError: (error) => toast.error(String(error)),
  });
  return (
    <form
      className="space-y-3 rounded-panel border border-border p-4"
      onSubmit={(event) => {
        event.preventDefault();
        save.mutate({ exchangeRates: { [currency]: rate.trim() } });
      }}
    >
      <h3 className="font-semibold">
        {t("scope.pricing.settingsTitle", "计价汇率")}
      </h3>
      <p className="text-caption text-fg-2">
        {t(
          "scope.pricing.fxNote",
          "非美元单价按最后保存的计价汇率折为美元入账，记录原币金额和汇率快照。此设置独立于显示汇率；保存只补齐未知费用，已有费用保持原值。",
        )}
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <label htmlFor="pricing-rate">1 USD =</label>
        <Input
          id="pricing-rate"
          aria-label={t("scope.pricing.rate", "计价汇率")}
          className="w-44"
          type="number"
          min="0.000000001"
          max="1000000000"
          step="any"
          required
          value={rate}
          onChange={(event) => setRate(event.target.value)}
          placeholder={t("scope.pricing.configureRate", "请输入计价汇率")}
        />
        <select
          aria-label={t("scope.pricing.currency", "计价币种")}
          className="rounded border border-border bg-surface px-2 py-1"
          value={currency}
          onChange={(event) => {
            const next = event.target.value;
            setCurrency(next);
            setRate(settings.exchangeRates[next] ?? "");
          }}
        >
          {["CNY", "EUR", "JPY", "TWD", "TRY"].map((code) => (
            <option key={code}>{code}</option>
          ))}
        </select>
        <Button
          size="compact"
          type="submit"
          disabled={save.isPending || reprice.isPending}
        >
          {t("common.save", "保存")}
        </Button>
      </div>
      {!settings.exchangeRates.CNY && (
        <p className="text-caption text-fg-2">
          {t(
            "scope.pricing.missingCny",
            "DeepSeek 使用人民币官方价；配置人民币汇率后才能折为美元。",
          )}
        </p>
      )}
      <details className="text-caption">
        <summary className="cursor-pointer text-fg-2">
          {t("scope.pricing.calendar", "DeepSeek 节假日日历（北京时间）")}
        </summary>
        <p className="my-2 text-fg-2">
          {t(
            "scope.pricing.schedule",
            "周一至周五 09:00–12:00、14:00–18:00 为高峰；周末和中国节假日为空闲，调休周末仍为空闲。未配置年度日历时保留未定价。",
          )}
        </p>
        <label className="flex items-center gap-2">
          {t("scope.pricing.year", "年份")}
          <Input
            aria-label={t("scope.pricing.year", "年份")}
            className="w-28"
            type="number"
            min="2000"
            max="2100"
            value={year}
            onChange={(event) => {
              const next = event.target.value;
              setYear(next);
              setDates((settings.holidayCalendars[next] ?? []).join("\n"));
            }}
          />
        </label>
        <label className="mt-2 block">
          {t(
            "scope.pricing.holidays",
            "节假日日期，每行一个 YYYY-MM-DD（空列表表示确认该年没有节假日）",
          )}
          <textarea
            aria-label={t("scope.pricing.holidayDates", "节假日日期")}
            className="mt-1 h-28 w-full rounded border border-border bg-surface p-2 font-mono"
            value={dates}
            onChange={(event) => setDates(event.target.value)}
          />
        </label>
        <Button
          className="my-2"
          type="button"
          variant="neutral"
          size="compact"
          disabled={save.isPending || reprice.isPending}
          onClick={() =>
            save.mutate({
              holidayCalendars: {
                [year]: dates
                  .split(/[\s,]+/)
                  .filter(Boolean)
                  .sort(),
              },
            })
          }
        >
          {t("scope.pricing.saveCalendar", "保存该年度日历")}
        </Button>
        <br />
        <a
          className="text-link"
          href="https://www.gov.cn/zhengce/content/202511/content_7047090.htm"
          target="_blank"
          rel="noreferrer"
        >
          {t(
            "scope.pricing.calendarSource",
            "内置 2026 年日历来源：中国政府网",
          )}
        </a>
      </details>
      <div className="flex flex-wrap items-center gap-2 border-t border-border pt-3">
        <Button
          type="button"
          variant="neutral"
          size="compact"
          disabled={save.isPending || reprice.isPending}
          onClick={() => reprice.mutate()}
        >
          {t("scope.pricing.recalculate", "按已保存规则重算本机估算费用")}
        </Button>
        <span className="text-caption text-fg-3">
          {t(
            "scope.pricing.recalculateNote",
            "仅更新本机会话估算；原生账单、历史迁移及远端记录保持原值。",
          )}
        </span>
      </div>
    </form>
  );
}

export function PricingSettingsPanel() {
  const query = usePricingSettings();
  if (!query.data)
    return query.isError ? <p role="alert">{String(query.error)}</p> : null;
  return <SettingsForm key={query.data.updatedAt} settings={query.data} />;
}
