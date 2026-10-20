import type { GoCatalogInfo } from "@/types/goCatalog";
export function goQueryPrompt(info: GoCatalogInfo) {
  return `请核实并更新 Harness Scope 的 OpenCode Go 本地额度规则。

官方来源：${info.catalog.sourceUrl}
目标本地 JSON：${info.path}
当前配置版本：${info.catalog.version}；核查日期：${info.catalog.checkedAt}。

请同时查看 Go 与 Go Plus 两个 Usage limits 表，以及 DeepSeek 的高峰说明和上下文分档。/inference/go/v1/models 只有模型 ID，不能用它或 models.dev 推断套餐额度。逐模型记录 USD / 100万 Tokens 的 input、output、cacheRead、cacheWrite，以及 Go / Go Plus 的 monthlyLimits。monthlyLimits 是模型额度，不是月费。免费模型标 free=true、两种额度为 null，所有实际免费 Token 单价为 "0"。未公布的缓存写入或 1h 写入单价保留 null，不能当成零价，也不要自行乘倍率。

保留 schemaVersion=1，更新 version、checkedAt 为实际核查日期，并保留官方 sourceUrl。核对 windowFractions（目前五小时 0.2、每周 0.5、每月 1）。peakSchedule 目前为 UTC 周一至周五 01:00–04:00、06:00–10:00，分钟区间为 [60,240]、[360,600]；以最新官方说明为准，不能套用中国节假日。peakPrices 必须记录官方高峰单价，不要假定永远是非高峰的两倍。

每个模型的 tiers 按上下文上限递增排列，最后一档 maxContextTokens=null。上下文采用未缓存输入 + 缓存读取 + 缓存写入，K 按 1000 Tokens 表示；如官方明确给出其他阈值则据此修正。DeepSeek Flash 的计价 ID 保持 deepseek-flash，在 aliases 中包含官方 deepseek-v4.1-flash；不要改动日志里的原始模型名。其他 ID 按官方模型目录核对。仅在官方公布了典型 Token 数时填写 sampleTokens，否则保留 null。保留限时优惠、免费限制等 notes；页面缺失或含糊的内容不要编造。核实不到的原条目请保留并在 notes 注明待核验。

仅更新这个 JSON 文件，不读取或修改 API Key、凭据文件、账号订阅、会话日志或 SQLite 数据库。先读取现有文件；若不存在则以本文末尾的当前配置为起点。用合法 UTF-8 JSON 原子替换文件，并报告更改项、依据链接和仍待核验的内容。此文件是待导入配置，不会自动重算历史费用。完成后提示用户在供应商页点击“从文件载入”，预览后“校验并应用”。若你没有访问本机文件的能力，只返回完整 JSON 供粘贴导入。

当前完整配置：
\`\`\`json
${JSON.stringify(info.catalog, null, 2)}
\`\`\``;
}
export function goPercent(value: string) {
  const n = Number(value);
  if (!Number.isFinite(n)) return "—";
  if (n > 0 && n < 0.0001) return "<0.0001%";
  return `${Number(n.toFixed(4))}%`;
}
export function goError(error: unknown, zh: boolean) {
  const message = String(error);
  if (!zh) return message;
  if (/changed; reload/.test(message))
    return "配置或文件已被其他操作修改，请重新载入后再保存。";
  if (/cache-write.*unavailable/.test(message))
    return "当前配置没有该缓存写入时长的单价，无法完整估算。";
  if (
    /Go.*(decimal|price|context|alias|catalog|version|tier|fractions|weekday|schedule|allowance|metadata|JSON)/i.test(
      message,
    )
  )
    return `Go 规则校验失败：${message}`;
  return message;
}
