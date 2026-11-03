import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { OptionSelect } from "@/components/ui/option-select";
import type { RequestPriceTier } from "@/types/usage";

export function RequestTariffEditor({ tiers, onChange, enabled }: {
  tiers: RequestPriceTier[]; onChange: (value: RequestPriceTier[]) => void; enabled: boolean;
}) {
  const {i18n}=useTranslation();
  const l=(zh:string,en:string)=>i18n.language.startsWith("zh")?zh:en;
  const fields=[
    ["inputCostPerMillion",l("输入","Input")], ["outputCostPerMillion",l("输出","Output")],
    ["cacheReadCostPerMillion",l("缓存读取","Cache read")], ["cacheCreationCostPerMillion",l("缓存写入","Cache write")],
  ] as const;
  const update=(index:number,patch:Partial<RequestPriceTier>)=>onChange(tiers.map((t,i)=>i===index?{...t,...patch}:t));
  const date=(seconds?:number|null)=>seconds == null ? "" : new Date(seconds*1000).toISOString().slice(0,16);
  return <fieldset className="space-y-3 rounded-panel border border-border p-3" disabled={!enabled}>
    <legend className="px-1 font-medium">{l("供应商条件价格","Provider conditional prices")}</legend>
    <p className="text-caption text-fg-2">{l(
      "填写供应商 ID 后可添加绝对单价，不使用统一 Fast 倍率。匹配速度优先，其次取更高输入阈值，同条件按列表顺序。输入阈值含缓存；未匹配时使用基础价。已冻结费用不变。",
      "Set a provider ID to add absolute rates, without a global Fast multiplier. Speed-specific rules take priority, then higher prompt thresholds, then list order. Prompt totals include cache. Unmatched requests use base rates. Frozen costs stay unchanged.",
    )}</p>
    {tiers.map((tier,index)=><div key={tier.id} className="space-y-2 rounded border border-border p-3">
      <div className="flex items-end gap-3">
        <label className="flex-1 text-caption">{l("请求速度","Request speed")}
          <OptionSelect value={tier.speed ?? "any"} onValueChange={v=>update(index,{speed:v==="any"?null:v as "fast"|"standard"})}>
            <option value="any">{l("不限","Any")}</option><option value="fast">Fast</option><option value="standard">Standard</option>
          </OptionSelect>
        </label>
        <label className="flex-1 text-caption">{l("输入超过 Token（0 为不限）","Prompt tokens above (0: any)")}
          <Input type="number" min={0} step={1} value={tier.abovePromptTokens} onChange={e=>update(index,{abovePromptTokens:Number(e.target.value)})}/>
        </label>
        <Button type="button" variant="ghost" onClick={()=>onChange(tiers.filter((_,i)=>i!==index))}>{l("移除","Remove")}</Button>
      </div>
      <div className="grid grid-cols-2 gap-3">{fields.map(([key,label])=><label key={key} className="text-caption">{label} / 1M
        <Input required type="number" min="0" step="any" value={tier[key]} onChange={e=>update(index,{[key]:e.target.value})}/>
      </label>)}</div>
      <div className="grid grid-cols-2 gap-3">{(["effectiveFrom","effectiveUntil"] as const).map(key=><label key={key} className="text-caption">
        {key==="effectiveFrom"?l("生效起点 UTC（可选）","Effective from UTC (optional)"):l("失效时间 UTC，不含（可选）","Until UTC, exclusive (optional)")}
        <Input type="datetime-local" value={date(tier[key])} onChange={e=>update(index,{[key]:e.target.value?Date.parse(`${e.target.value}Z`)/1000:null})}/>
      </label>)}</div>
    </div>)}
    <Button type="button" variant="neutral" onClick={()=>onChange([...tiers,{id:crypto.randomUUID(),speed:"fast",abovePromptTokens:0,inputCostPerMillion:"0",outputCostPerMillion:"0",cacheReadCostPerMillion:"0",cacheCreationCostPerMillion:"0"}])}>{l("添加条件价格","Add conditional price")}</Button>
  </fieldset>;
}
