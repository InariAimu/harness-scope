import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input, fieldClass } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import { OptionSelect } from "@/components/ui/option-select";
import { providerAccountsApi } from "@/lib/api/providerAccounts";
import { accountError } from "@/lib/providerAccounts";
import type { CustomQuerySettings, CustomUsage } from "@/types/providerAccounts";
import { CustomUsagePanel } from "./CustomUsagePanel";

const GENERAL = `({
  request: {
    url: "{{baseUrl}}/user/balance",
    method: "GET",
    headers: { Authorization: "Bearer {{apiKey}}" }
  },
  extractor(response) {
    return {
      id: "balance",
      planName: "Balance",
      remaining: response.balance,
      unit: "USD"
    };
  }
})`;
const NEW_API = `({
  request: {
    url: "{{baseUrl}}/api/user/self",
    method: "GET",
    headers: {
      Authorization: "Bearer {{apiKey}}",
      "New-Api-User": "YOUR_USER_ID"
    }
  },
  extractor(response) {
    if (!response.success || !response.data) return { isValid: false };
    const quotaPerUnit = 500000; // Set this to your service's conversion rate.
    return {
      id: "balance",
      planName: response.data.group || "Balance",
      remaining: String(response.data.quota / quotaPerUnit),
      used: String(response.data.used_quota / quotaPerUnit),
      unit: "USD"
    };
  }
})`;
export const defaultCustomQuery = (): CustomQuerySettings => ({ enabled: false, baseUrl: "", script: GENERAL, timeoutSeconds: 10 });

export function CustomQueryEditor({ value, onChange, id, apiKey, clearApiKey, zh, timezone, onTesting }: {
  value: CustomQuerySettings; onChange: (v: CustomQuerySettings) => void; id?: string; apiKey: string; clearApiKey: boolean; zh: boolean; timezone: string; onTesting: (v: boolean) => void;
}) {
  const [testing, setTesting] = useState(false);
  const [result, setResult] = useState<{ input: string; data?: CustomUsage; error?: string }>();
  const l = (a: string, b: string) => zh ? a : b;
  const fingerprint = JSON.stringify([value, id, apiKey, clearApiKey]);
  const test = async () => {
    setTesting(true); onTesting(true);
    try {
      const data = await providerAccountsApi.testQuery({ id, settings: value, apiKey: apiKey.trim() || undefined, clearApiKey });
      setResult({ input: fingerprint, data });
    } catch (e) { setResult({ input: fingerprint, error: accountError(String(e), zh) }); }
    finally { setTesting(false); onTesting(false); }
  };
  return <section className="border-t border-border pt-4 space-y-3 min-w-0">
    <label className="flex items-center gap-2 font-semibold text-fg-1">
      <Checkbox checked={value.enabled} onCheckedChange={enabled => onChange({ ...value, enabled })} />
      {l("启用自定义用量查询", "Enable custom usage query")}
    </label>
    <p className="text-caption text-fg-3">{l("关闭时仍可显示供应商并关联 provider ID。", "Provider display and ID links remain available when queries are disabled.")}</p>
    {value.enabled && <>
      <label className="block space-y-1.5 text-caption text-fg-2">Base URL
        <Input aria-label="Base URL" type="url" value={value.baseUrl} placeholder="https://api.example.com" required maxLength={2048} onChange={e => onChange({ ...value, baseUrl: e.target.value })} />
      </label>
      <div className="flex items-end gap-3">
        <label className="flex-1 space-y-1.5 text-caption text-fg-2">{l("填入模板", "Insert template")}
          <OptionSelect aria-label={l("查询模板", "Query template")} className="scope-select w-full" value="custom" onValueChange={template => {
            if (template === "custom") return;
            if (value.script.trim() && value.script !== GENERAL && !window.confirm(l("用模板替换当前脚本？", "Replace the current script with this template?"))) return;
            onChange({ ...value, script: template === "newapi" ? NEW_API : GENERAL });
          }}>
            <option value="custom">{l("自定义", "Custom")}</option><option value="general">{l("通用余额", "Generic balance")}</option><option value="newapi">New API</option>
          </OptionSelect>
        </label>
        <label className="w-28 space-y-1.5 text-caption text-fg-2">{l("超时（秒）", "Timeout (s)")}
          <Input aria-label={l("查询超时", "Query timeout")} type="number" min={2} max={30} required value={value.timeoutSeconds} onChange={e => onChange({ ...value, timeoutSeconds: Number(e.target.value) })} />
        </label>
      </div>
      <label className="block space-y-1.5 text-caption text-fg-2">{l("查询脚本", "Query script")}
        <textarea aria-label={l("查询脚本", "Query script")} className={`${fieldClass} min-h-72 resize-y font-mono text-xs p-2`} spellCheck={false} value={value.script} maxLength={32768} required onChange={e => onChange({ ...value, script: e.target.value })} />
      </label>
      <p className="text-caption text-fg-3 break-words">{l("{{apiKey}} 使用上方凭据，{{baseUrl}} 使用查询地址。New API 请修改用户 ID 和额度换算系数。", "{{apiKey}} uses the credential above; {{baseUrl}} uses the query address. For New API, set your user ID and quota conversion rate.")}</p>
      <details className="text-caption text-fg-3">
        <summary className="cursor-pointer">{l("返回格式", "Result format")}</summary>
        <p className="mt-2 break-words">{l("返回一个对象或数组。每项包含 id、planName、remaining／used／total、unit；金额推荐返回字符串。可选 usedPercent、resetsAt（秒或含时区的 ISO 时间）、durationMinutes、windowType（fixed／rolling）。仅 fixed 窗口推算满额起点。", "Return an object or array with stable id, planName, remaining / used / total and unit. Prefer decimal strings for money. Optional: usedPercent, resetsAt (Unix seconds or ISO time with offset), durationMinutes and windowType (fixed / rolling). Only fixed windows derive a full-quota start.")}</p>
      </details>
      <Button type="button" variant="neutral" disabled={testing || !value.baseUrl.trim() || !value.script.trim()} onClick={() => void test()}>{testing ? l("测试中…", "Testing…") : l("测试查询", "Test query")}</Button>
      <p className="text-caption text-fg-3">{l("测试使用当前草稿，结果不写入余额历史。保存后按刷新设置记录。", "Tests use this draft without adding history. Saved queries record results on refresh.")}</p>
      {result?.input === fingerprint && result.error && <p role="alert" className="text-caption text-danger-text break-words">{result.error}</p>}
      {result?.input === fingerprint && result.data && <CustomUsagePanel data={result.data} zh={zh} locale={zh ? "zh-CN" : "en-US"} timezone={timezone} />}
    </>}
  </section>;
}
