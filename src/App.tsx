import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  Activity,
  ChartColumn,
  History,
  LayoutGrid,
  Settings,
  RefreshCw,
  ChevronsLeft,
  ChevronsRight,
} from "lucide-react";
import { command, setScope } from "@/lib/client";
import { toast } from "@/lib/toast";
import { hasUnsavedChanges } from "@/lib/unsavedChanges";
import { UsageDashboard } from "@/components/usage/UsageDashboard";
import { ApplicationsPage } from "@/pages/ApplicationsPage";
import { SettingsPage } from "@/pages/SettingsPage";
import { SessionsPage } from "@/pages/SessionsPage";
import { syncModelsDevPricingOnStartup } from "@/lib/modelsDevAutoSync";
import { Button } from "@/components/ui/button";

export interface Source {
  id: string;
  harness: string;
  root: string;
  enabled: boolean;
}
export interface Config {
  deviceName: string;
  sources: Source[];
  serverUrl: string;
  serverToken: string;
  scanIntervalSeconds: number;
  theme: string;
  language: string;
  timezone: string;
  modelsDevSync: unknown;
}
export interface Snapshot {
  deviceId: string;
  config: Config;
  tokenConfigured: boolean;
  harnesses: {
    id: string;
    name: string;
    parserVersion: string;
    defaultRoots: string[];
  }[];
  pending: number;
  devices: { id: string; name: string }[];
  lastScanAt: number | null;
  lastScan: {
    imported: number;
    updated: number;
    filesScanned: number;
    errors: string[];
    deferredFiles: number;
  } | null;
  lastSyncAt: number | null;
  lastSyncError: string | null;
  migrations: unknown[];
}
export default function App() {
  const { t, i18n } = useTranslation();
  const client = useQueryClient();
  const state = useQuery({
    queryKey: ["snapshot"],
    queryFn: () => command<Snapshot>("snapshot"),
    refetchInterval: 10000,
  });
  const [page, setPage] = useState("usage");
  const [collapsed, setCollapsed] = useState(false);
  const [device, setDevice] = useState("local");
  const [busy, setBusy] = useState(false);
  const data = state.data;
  useEffect(() => {
    if (!data) return;
    const c = data.config;
    document.documentElement.classList.toggle(
      "dark",
      c.theme === "dark" ||
        (c.theme === "system" &&
          matchMedia("(prefers-color-scheme: dark)").matches),
    );
    void i18n.changeLanguage(c.language);
    localStorage.setItem("harness-scope.language", c.language);
  }, [data?.config.theme, data?.config.language, i18n]);
  useEffect(() => {
    if (data) {
      void syncModelsDevPricingOnStartup()
        .then(() => client.invalidateQueries({ queryKey: ["usage"] }))
        .catch((e) => toast.error(String(e)));
    }
  }, [!!data, client]);
  const refresh = async () => {
    setBusy(true);
    try {
      const r = await command<{
        imported: number;
        updated: number;
        errors: string[];
      }>("scan");
      window.dispatchEvent(new Event("harness-scope:changed"));
      await client.invalidateQueries();
      if (r.errors.length) toast.warning(r.errors[0]);
      else
        toast.success(
          `${t("scope.imported", "新增")} ${r.imported} · ${t("scope.updated", "修订")} ${r.updated}`,
        );
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };
  const navigate = (next: string) => {
    if (
      hasUnsavedChanges() &&
      !window.confirm(t("scope.unsaved", "放弃未保存的修改？"))
    )
      return;
    setPage(next);
  };
  const label = (zh: string, en: string) =>
    i18n.language.startsWith("zh") ? zh : en;
  return (
    <div className="scope-shell">
      <aside className={`scope-sidebar ${collapsed ? "is-collapsed" : ""}`}>
        <div className="scope-brand">
          <Activity size={19} className="text-action-text" />
          {!collapsed && <strong>Harness Scope</strong>}
          <button
            aria-label="Toggle sidebar"
            onClick={() => setCollapsed((v) => !v)}
            className="ml-auto p-1 text-fg-3"
          >
            {collapsed ? (
              <ChevronsRight size={15} />
            ) : (
              <ChevronsLeft size={15} />
            )}
          </button>
        </div>
        <div className="px-3 py-5 text-caption text-fg-3">
          {collapsed ? "HS" : label("本机工具", "LOCAL TOOLS")}
        </div>
        <nav className="flex-1 px-2 space-y-1">
          {[
            {
              id: "applications",
              name: label("应用", "Applications"),
              Icon: LayoutGrid,
            },
            {
              id: "usage",
              name: label("用量统计", "Usage"),
              Icon: ChartColumn,
            },
            { id: "sessions", name: label("会话", "Sessions"), Icon: History },
          ].map(({ id, name, Icon }) => (
            <button
              key={id}
              title={name}
              className={`scope-nav ${page === id ? "active" : ""}`}
              onClick={() => navigate(id)}
            >
              <Icon size={16} />
              {!collapsed && name}
            </button>
          ))}
        </nav>
        <div className="border-t border-border p-2">
          <button
            title={label("设置", "Settings")}
            className={`scope-nav ${page === "settings" ? "active" : ""}`}
            onClick={() => navigate("settings")}
          >
            <Settings size={16} />
            {!collapsed && label("设置", "Settings")}
          </button>
          {!collapsed && (
            <div className="px-3 pt-3 pb-2 text-caption text-fg-3">
              v0.1.0 · {data?.config.deviceName || "Local"}
            </div>
          )}
        </div>
      </aside>
      <main className="scope-main">
        {state.isError ? (
          <div className="m-8 rounded-panel border border-border p-6">
            <h1>
              {label("无法连接本地服务", "Cannot connect to the local service")}
            </h1>
            <p className="mt-3 text-fg-2">{String(state.error)}</p>
            <code className="block mt-4">
              cargo run -p harness-scope-service -- --local --bind
              127.0.0.1:8788
            </code>
            <Button className="mt-4" onClick={() => state.refetch()}>
              {label("重试", "Retry")}
            </Button>
          </div>
        ) : !data ? (
          <div className="p-8">
            {label("正在读取本地数据…", "Loading local data…")}
          </div>
        ) : (
          <>
            {page === "usage" && (
              <>
                <div className="scope-devicebar">
                  <label
                    className="text-caption text-fg-2"
                    htmlFor="scope-device"
                  >
                    {label("统计范围", "Devices")}
                  </label>
                  <select
                    id="scope-device"
                    value={device}
                    onChange={(e) => {
                      setDevice(e.target.value);
                      setScope(e.target.value);
                      void client.invalidateQueries({ queryKey: ["usage"] });
                    }}
                    className="scope-select"
                  >
                    <option value="local">
                      {label("本机", "This device")}
                    </option>
                    <option value="all">
                      {label("全部设备", "All devices")}
                    </option>
                    {data.devices
                      .filter((d) => d.id !== data.deviceId)
                      .map((d) => (
                        <option key={d.id} value={d.id}>
                          {d.name}
                        </option>
                      ))}
                  </select>
                  <span className="flex-1" />
                  <span className="text-caption text-fg-3">
                    {data.pending} {label("条待同步", "pending")}
                    {device !== "local" &&
                      ` · ${label("远端为最近同步缓存", "remote data is cached")}`}
                  </span>
                </div>
                <UsageDashboard key={device} refreshIntervalMs={60000} />
                <p className="px-6 pb-4 text-caption text-fg-3">
                  {label(
                    "会话记录不提供真实 HTTP 成功率或精确生成速度。费用为日志自报、价表估算或历史导入值；旧日汇总没有请求明细。",
                    "Session logs do not provide HTTP success rates or exact generation speed. Costs retain their reported, estimated or imported provenance; legacy daily totals have no request details.",
                  )}
                </p>
              </>
            )}
            {page === "applications" && (
              <ApplicationsPage
                snapshot={data}
                onChanged={() => client.invalidateQueries()}
              />
            )}{" "}
            {page === "settings" && (
              <SettingsPage
                snapshot={data}
                onChanged={() => client.invalidateQueries()}
              />
            )}{" "}
            {page === "sessions" && (
              <SessionsPage onSettings={() => navigate("settings")} />
            )}
          </>
        )}
      </main>
      <div className="scope-status">
        <span className={`scope-dot ${state.isError ? "error" : ""}`} />
        {data?.config.deviceName || "Harness Scope"}
        <span className="mx-2 text-fg-3">·</span>
        {data?.lastScanAt
          ? new Date(data.lastScanAt).toLocaleTimeString()
          : label("等待首次采集", "Awaiting first scan")}
        <span className="flex-1" />
        {data?.lastSyncError && (
          <span
            className="text-danger-text truncate max-w-[50%]"
            title={data.lastSyncError}
          >
            {data.lastSyncError}
          </span>
        )}
        <button
          className="inline-flex gap-1 items-center px-2"
          disabled={busy || !data}
          onClick={() => void refresh()}
        >
          <RefreshCw size={12} className={busy ? "animate-spin" : ""} />
          {label("采集", "Scan")}
        </button>
      </div>
    </div>
  );
}
