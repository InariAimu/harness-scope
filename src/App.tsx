import { OptionSelect } from "@/components/ui/option-select";
import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import {
  Activity,
  ChartColumn,
  History,
  LayoutGrid,
  Settings,
  Wallet,
  ChevronsLeft,
  ChevronsRight,
} from "lucide-react";
import { command, setScope } from "@/lib/client";
import { toast } from "@/lib/toast";
import { hasUnsavedChanges } from "@/lib/unsavedChanges";
import { UsageDashboard } from "@/components/usage/UsageDashboard";
import { SidebarToday } from "@/components/shell/SidebarToday";
import { ApplicationsPage } from "@/pages/ApplicationsPage";
import { SettingsPage } from "@/pages/SettingsPage";
import { SessionsPage } from "@/pages/SessionsPage";
import { ProvidersPage } from "@/pages/ProvidersPage";
import { syncModelsDevPricingOnStartup } from "@/lib/modelsDevAutoSync";
import { Button } from "@/components/ui/button";
import { isTauri } from "@tauri-apps/api/core";
import { isWindows, DRAG_REGION_ATTR } from "@/lib/platform";
import {
  AppPageHeader,
  WindowControlsContext,
} from "@/components/shell/AppPageHeader";
import { WindowControls } from "@/components/shell/WindowControls";
import { AccentThemeProvider } from "@/components/theme/AccentThemeProvider";
import { InterfaceSkinBackdrop } from "@/components/theme/InterfaceSkinBackdrop";

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
  accentColor: string;
  interfaceSkin: "none" | "deepseek";
  skinTransparency: number;
  skinMaskStrength: number;
  language: string;
  timezone: string;
  modelsDevSync: unknown;
}
export interface Snapshot {
  usageVersion: number;
  deviceId: string;
  config: Config;
  tokenConfigured: boolean;
  harnesses: {
    id: string;
    name: string;
    parserVersion: string;
    defaultRoots: string[];
    usage: boolean;
    transcript: boolean;
    delete: boolean;
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
    refetchInterval: 5000,
  });
  const [page, setPage] = useState("usage");
  const [collapsed, setCollapsed] = useState(false);
  const [device, setDevice] = useState("local");
  const data = state.data;
  const usageVersion = useRef<number | undefined>();
  useEffect(() => {
    if (!data) return;
    if (
      usageVersion.current !== undefined &&
      usageVersion.current !== data.usageVersion
    ) {
      void client.invalidateQueries({ queryKey: ["usage"] });
    }
    usageVersion.current = data.usageVersion;
  }, [data?.usageVersion, client]);
  useEffect(() => {
    if (!data) return;
    const c = data.config;
    void i18n.changeLanguage(c.language);
    localStorage.setItem("harness-scope.language", c.language);
  }, [data?.config.language, i18n]);
  useEffect(() => {
    if (data) {
      void syncModelsDevPricingOnStartup(data.lastScanAt)
        .then((result) => {
          if (result.changed > 0)
            void client.invalidateQueries({ queryKey: ["usage", "pricing"] });
          if (result.usageChanged > 0)
            void client.invalidateQueries({ queryKey: ["snapshot"] });
        })
        .catch((e) => toast.error(String(e)));
    }
  }, [!!data, data?.lastScanAt, client]);
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
    <AccentThemeProvider
      color={data?.config.accentColor}
      theme={data?.config.theme}
      skin={data?.config.interfaceSkin}
      skinTransparency={data?.config.skinTransparency}
      skinMaskStrength={data?.config.skinMaskStrength}
    >
    <WindowControlsContext.Provider
      value={isTauri() && isWindows() ? <WindowControls /> : null}
    >
      <div className="scope-shell">
        <aside className={`scope-sidebar ${collapsed ? "is-collapsed" : ""}`}>
          <div className="scope-brand" {...DRAG_REGION_ATTR}>
            <Activity size={19} className="text-action-text" />
            {!collapsed && <strong {...DRAG_REGION_ATTR}>Harness Scope</strong>}
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
              {
                id: "sessions",
                name: label("会话", "Sessions"),
                Icon: History,
              },
              {
                id: "providers",
                name: label("供应商", "Providers"),
                Icon: Wallet,
              },
            ].map(({ id, name, Icon }) => (
              <button
                key={id}
                title={name}
                aria-label={name}
                className={`scope-nav ${page === id ? "active" : ""}`}
                onClick={() => navigate(id)}
              >
                <Icon size={16} />
                {!collapsed && name}
                {!collapsed && id === "usage" && <SidebarToday />}
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
          <InterfaceSkinBackdrop />
          {state.isError ? (
            <>
              <AppPageHeader title="Harness Scope" />
              <div className="m-8 rounded-panel border border-border p-6">
                <h1>
                  {label(
                    "无法连接本地服务",
                    "Cannot connect to the local service",
                  )}
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
            </>
          ) : !data ? (
            <>
              <AppPageHeader title="Harness Scope" />
              <div className="p-8">
                {label("正在读取本地数据…", "Loading local data…")}
              </div>
            </>
          ) : (
            <>
              {page === "usage" && (
                <UsageDashboard
                  backgroundCollection
                  key={device}
                  refreshIntervalMs={data.config.scanIntervalSeconds * 1000}
                  onRefreshIntervalChange={async (next) => {
                    try {
                      await command("save_config", {
                        config: {
                          ...data.config,
                          scanIntervalSeconds: next / 1000,
                        },
                      });
                      await client.invalidateQueries({
                        queryKey: ["snapshot"],
                      });
                      return true;
                    } catch (error) {
                      toast.error(String(error));
                      return false;
                    }
                  }}
                  deviceControls={
                    <div className="scope-devicebar">
                      <label
                        className="text-caption text-fg-2"
                        htmlFor="scope-device"
                      >
                        {label("统计范围", "Devices")}
                      </label>
                      <OptionSelect
                        id="scope-device"
                        value={device}
                        onValueChange={(value) => {
                          setDevice(value);
                          setScope(value);
                          void client.invalidateQueries({
                            queryKey: ["usage"],
                          });
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
                      </OptionSelect>
                      <span className="flex-1" />
                      <span className="text-caption text-fg-3">
                        {data.pending} {label("条待同步", "pending")}
                        {device !== "local" &&
                          ` · ${label("远端为最近同步缓存", "remote data is cached")}`}
                      </span>
                    </div>
                  }
                />
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
              {page === "providers" && <ProvidersPage snapshot={data} />}
            </>
          )}
        </main>
      </div>
    </WindowControlsContext.Provider>
    </AccentThemeProvider>
  );
}
