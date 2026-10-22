import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { RefreshCw, Save } from "lucide-react";
import { command } from "@/lib/client";
import { toast } from "@/lib/toast";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { OptionSelect } from "@/components/ui/option-select";

interface Settings {
  mode: "system" | "manual" | "direct";
  proxyUrl: string;
}
interface NetworkState {
  settings: Settings;
  routes: { id: string; configured: boolean; invalid: boolean; proxy: string | null }[];
}

export function NetworkSettings() {
  const { i18n } = useTranslation();
  const l = (zh: string, en: string) => i18n.language.startsWith("zh") ? zh : en;
  const client = useQueryClient();
  const query = useQuery({ queryKey: ["network-settings"], queryFn: () => command<NetworkState>("network_settings") });
  const [draft, setDraft] = useState<Settings>();
  const [busy, setBusy] = useState(false);
  const [test, setTest] = useState<string>();
  const settings = draft ?? query.data?.settings;
  const update = (patch: Partial<Settings>) => {
    if (settings) setDraft({ ...settings, ...patch });
    setTest(undefined);
  };
  const run = async (save: boolean) => {
    if (!settings) return;
    setBusy(true);
    setTest(undefined);
    try {
      if (save) {
        const state = await command<NetworkState>("save_network_settings", { settings });
        client.setQueryData(["network-settings"], state);
        setDraft(undefined);
        toast.success(l("网络设置已保存，下次请求生效", "Network settings saved; applies to new requests"));
      } else {
        const result = await command<{ elapsedMs: number }>("test_network_settings", { settings });
        setTest(l(`models.dev 连接成功 · ${result.elapsedMs} ms`, `Connected to models.dev · ${result.elapsedMs} ms`));
      }
    } catch (error) {
      toast.error(String(error));
      if (!save) setTest(l("连接失败，请检查代理地址和代理服务。", "Connection failed. Check the proxy address and service."));
    } finally {
      setBusy(false);
    }
  };
  if (!settings) return <section className="scope-card text-fg-2">
    {query.isError ? <Button variant="neutral" onClick={() => void query.refetch()}>{l("加载失败，重试", "Load failed; retry")}</Button> : l("正在读取网络设置…", "Loading network settings…")}
  </section>;
  const names: Record<string, string> = {
    sync: l("多设备同步", "Device sync"), modelsDev: "models.dev",
    deepseek: l("DeepSeek 余额", "DeepSeek balance"), opencodeGo: l("OpenCode Go 额度", "OpenCode Go quota"),
    codex: l("Codex 额度查询", "Codex quota query"),
  };
  return <section className="scope-card space-y-5">
    <h2 className="font-semibold">{l("网络", "Network")}</h2>
    <p className="text-caption text-fg-3">{l(
      "统一用于同步、供应商余额／额度查询和 models.dev 下载。保存后对新请求生效。",
      "Shared by sync, provider balance/quota queries and models.dev downloads. Saving applies to new requests.",
    )}</p>
    <label className="scope-field">
      <span>{l("出站连接", "Outbound connection")}</span>
      <OptionSelect disabled={busy} value={settings.mode} onValueChange={(mode) => update({ mode: mode as Settings["mode"] })}>
        <option value="system">{l("跟随系统", "Follow system")}</option>
        <option value="manual">{l("指定代理", "Use proxy")}</option>
        <option value="direct">{l("直连", "Direct")}</option>
      </OptionSelect>
    </label>
    {settings.mode === "manual" && <label className="scope-field">
      <span>{l("代理地址", "Proxy address")}</span>
      <Input disabled={busy} value={settings.proxyUrl} onChange={(event) => update({ proxyUrl: event.target.value })}
        placeholder="http://127.0.0.1:7890" autoComplete="off" spellCheck={false} />
      <span className="text-caption text-fg-3">HTTP / HTTPS / SOCKS5 / SOCKS5H</span>
    </label>}
    <p className="text-caption text-fg-3">{settings.mode === "system" ? l(
      "优先读取代理环境变量，再读取系统手动代理；保留代理绕过规则。PAC 自动配置请改用指定代理。",
      "Uses proxy environment variables first, then the system manual proxy, respecting bypass rules. For PAC setups, specify a proxy instead.",
    ) : settings.mode === "manual" ? l(
      "使用无需认证的代理地址；本机回环连接直连。代理不可用时会报错，不会回退直连。",
      "Use a proxy without authentication. Loopback connections stay direct. An unavailable proxy fails without falling back to a direct connection.",
    ) : l("应用请求不使用代理。", "Application requests bypass proxies.")}</p>
    <div className="flex flex-wrap items-center gap-2">
      <Button variant="neutral" disabled={busy} onClick={() => void run(false)}>{l("测试连接", "Test connection")}</Button>
      <Button disabled={busy || (settings.mode === "manual" && !settings.proxyUrl.trim())} onClick={() => void run(true)}>
        <Save size={14} />{l("保存网络设置", "Save network settings")}
      </Button>
      {test && <span role="status" className="text-caption text-fg-2">{test}</span>}
    </div>
    <div className="border-t border-border pt-4">
      <div className="mb-2 flex items-center justify-between gap-2">
        <h3 className="text-caption font-semibold">{l("已保存配置的连接路径", "Routes for saved settings")}</h3>
        <Button variant="quiet" size="compact" disabled={query.isFetching || busy} onClick={() => void query.refetch()}><RefreshCw size={12} />{l("刷新", "Refresh")}</Button>
      </div>
      <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-5 gap-y-2 text-caption">
        {query.data?.routes.map((route) => <div key={route.id} className="contents">
          <dt className="text-fg-2">{names[route.id] ?? route.id}</dt>
          <dd className="min-w-0 break-all text-end font-mono text-fg-1">{!route.configured ? l("未配置", "Not configured") : route.invalid ? l("目标地址无效", "Invalid destination") : route.proxy ?? l("直连", "Direct")}</dd>
        </div>)}
      </dl>
      <p className="mt-3 text-caption text-fg-3">{l(
        "Codex 额度查询也使用此设置。外部浏览器打开的网页使用浏览器自身的网络设置。",
        "Codex quota queries also use these settings. Pages opened in an external browser use that browser's network settings.",
      )}</p>
    </div>
  </section>;
}
