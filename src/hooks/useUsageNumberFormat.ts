import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { getLocaleFromLanguage, getResolvedLang } from "@/components/usage/format";
import { useUsageDisplay } from "@/lib/query/usageDisplay";
import { createUsageNumberFormatter, resolveUsageNumberFormat } from "@/lib/usageNumberFormat";

/** Shared quantity formatting for charts, tables, sidebar and session summaries. */
export function useUsageNumberFormat() {
  const { data } = useUsageDisplay();
  const { i18n } = useTranslation();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const { unitStyle, decimals } = resolveUsageNumberFormat(data);
  return useMemo(() => createUsageNumberFormatter({
    numberUnitStyle: unitStyle,
    numberDecimals: decimals,
  }, locale), [unitStyle, decimals, locale]);
}
