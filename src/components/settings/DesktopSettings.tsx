import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Switch } from "@/components/ui/switch";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { toast } from "@/lib/toast";

interface Settings {
  launchOnStartup: boolean;
  silentStartup: boolean;
  minimizeToTray: boolean;
  trayAvailable: boolean;
  warning: string | null;
}

export function DesktopSettings() {
  const { i18n } = useTranslation();
  const l = (zh: string, en: string) => i18n.language.startsWith("zh") ? zh : en;
  const client = useQueryClient();
  const [busy, setBusy] = useState(false);
  const query = useQuery({ queryKey: ["desktop-settings"], queryFn: () => invoke<Settings>("desktop_settings"), staleTime: 0 });
  const update = async (key: "launchOnStartup" | "silentStartup" | "minimizeToTray", value: boolean) => {
    setBusy(true);
    try {
      const saved = await invoke<Settings>("save_desktop_settings", { patch: { [key]: value } });
      client.setQueryData(["desktop-settings"], saved);
    } catch (error) {
      toast.error(l("保存启动与关闭设置失败：", "Could not save startup and close settings: ") + String(error));
      void query.refetch();
    } finally { setBusy(false); }
  };
  return <section className="mb-6 space-y-3" aria-label={l("启动与关闭", "Startup and close")}>
    <h3 className="text-body font-medium">{l("启动与关闭", "Startup and close")}</h3>
    {query.data ? <>
      <div className="divide-y divide-border overflow-hidden rounded-control border border-border">
        {([
          ["launchOnStartup", l("开机自启", "Launch on startup"), l("登录 Windows 后自动运行 Harness Scope。", "Run Harness Scope when you sign in to Windows.")],
          ["silentStartup", l("静默启动", "Silent startup"), l("下次启动时不显示主窗口，仅在系统托盘运行。再次打开已运行的程序会显示窗口。", "On the next launch, start in the tray without showing the window. Opening an already running app restores its window.")],
          ["minimizeToTray", l("关闭时最小化到托盘", "Minimize to tray on close"), l("点击 X 或按 Alt+F4 隐藏窗口，保留页面并继续后台采集。关闭此选项后，关闭窗口将退出应用。", "X or Alt+F4 hides the window, preserving the page and background collection. When off, closing the window quits the app.")],
        ] as const).map(([key, title, help]) => <div key={key} className="flex items-center justify-between gap-4 px-4 py-3">
          <div className="flex items-center gap-2"><label htmlFor={key} className="text-body">{title}</label><HelpTip title={title}>{help}</HelpTip></div>
          <Switch id={key} aria-label={title} checked={query.data[key]} disabled={busy || (!query.data.trayAvailable && key !== "launchOnStartup")}
            onCheckedChange={(value) => void update(key, value)} />
        </div>)}
      </div>
      {!query.data.trayAvailable && <p role="alert" className="text-caption text-danger-text">{l("系统托盘不可用，窗口会正常显示，关闭窗口将退出应用。", "System tray is unavailable. The window stays accessible and closing it quits the app.")}</p>}
      {query.data.warning && <p role="alert" className="text-caption text-danger-text">{query.data.warning}</p>}
      <p role="status" className="text-caption text-fg-3">{busy ? l("正在保存…", "Saving…") : l("更改自动保存。静默启动在下次启动时生效。", "Changes save automatically. Silent startup applies on the next launch.")}</p>
    </> : query.isError ? <Button variant="neutral" onClick={() => void query.refetch()}>{l("读取失败，重试", "Load failed; retry")}</Button> : <p className="text-caption text-fg-3">{l("正在读取…", "Loading…")}</p>}
  </section>;
}
