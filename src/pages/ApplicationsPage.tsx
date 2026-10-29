import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Plus, Folder, Save, Trash2, LoaderCircle } from "lucide-react";
import { isTauri } from "@tauri-apps/api/core";
import type { Snapshot, Config } from "@/App";
import { command } from "@/lib/client";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { HoverTip } from "@/components/ui/hover-tip";
import { toast } from "@/lib/toast";
import { AppGlyph } from "@/components/shell/AppGlyph";
import { IconPickerPopover } from "@/components/icons/IconPickerPopover";
import { defaultHarnessIcon } from "@/lib/entityIcons";
import {
  applicationSettings,
  saveApplicationSettings,
  type ApplicationSettings,
} from "@/lib/applicationSettings";
export function ApplicationsPage({
  snapshot,
  onChanged,
}: {
  snapshot: Snapshot;
  onChanged: () => unknown;
}) {
  const { i18n } = useTranslation();
  const l = (zh: string, en: string) =>
    i18n.language.startsWith("zh") ? zh : en;
  const collectionDescription = l(
    "从已启用的目录采集用量；关闭后暂停后续采集，保留历史数据。",
    "Collect usage from enabled directories. Turning off pauses future scans and keeps history.",
  );
  const visibilityDescription = l(
    "在应用筛选项中显示；关闭不影响汇总、明细和会话列表。",
    "Show in application filters. Turning off keeps this app in totals, details and sessions.",
  );
  const [config, setConfig] = useState<Config>(snapshot.config);
  const [busy, setBusy] = useState(false);
  const client = useQueryClient();
  const [savingApp, setSavingApp] = useState<string | null>(null);
  const saveApp = async (harness: string, patch: Partial<ApplicationSettings>) => {
    setSavingApp(harness);
    try {
      await saveApplicationSettings(client, harness, patch);
    } catch (e) {
      toast.error(String(e));
    } finally {
      setSavingApp(null);
    }
  };
  const save = async () => {
    setBusy(true);
    try {
      await command("save_config", { config });
      await onChanged();
      toast.success(l("目录与采集设置已保存", "Sources saved"));
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };
  const browse = async (id: string) => {
    if (!isTauri()) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const root = await open({ directory: true, multiple: false });
    if (typeof root === "string")
      setConfig((c) => ({
        ...c,
        sources: c.sources.map((s) => (s.id === id ? { ...s, root } : s)),
      }));
  };
  return (
    <>
      <AppPageHeader
        title={l("应用", "Applications")}
        actions={
          <Button onClick={() => void save()} disabled={busy}>
            <Save size={14} />
            {l("保存目录", "Save directories")}
          </Button>
        }
      />
      <div className="scope-page space-y-5">
        <p className="text-caption text-fg-2">
          {l(
            "点击图标可更换；图标与应用开关自动保存，仅在本机生效。目录修改需点击「保存目录」。",
            "Click an icon to change it. Icons and switches save automatically on this device. Save directory changes with “Save directories”.",
          )}
        </p>
        {snapshot.harnesses.map((h) => (
          <section key={h.id} className="scope-card">
            <div className="flex items-start gap-3">
              <div className="flex min-w-0 flex-1 items-start gap-3">
                <IconPickerPopover target={{ kind: "harness", id: h.id }} name={h.name} fallback={defaultHarnessIcon(h.id)} className="scope-app-icon">
                  <AppGlyph app={h.id} size={24} fill badgeClassName="bg-surface" />
                </IconPickerPopover>
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <h2 className="min-w-0 break-words font-semibold">{h.name}</h2>
                    {savingApp === h.id && (
                      <span role="status" className="shrink-0 text-fg-3">
                        <LoaderCircle size={12} className="animate-spin" aria-hidden="true" />
                        <span className="sr-only">{l("保存中…", "Saving…")}</span>
                      </span>
                    )}
                  </div>
                  <p className="break-words text-caption text-fg-3">
                    {h.parserVersion} ·{" "}
                    {[
                      h.usage && l("用量采集", "Usage"),
                      h.transcript && l("本机会话", "Local sessions"),
                    ]
                      .filter(Boolean)
                      .join(" / ")}
                    {!h.delete &&
                      ` · ${l("请在原应用中删除会话", "Delete sessions in the original app")}`}
                  </p>
                </div>
              </div>
              <div className="flex shrink-0 items-center gap-4 pt-1">
                {/* Tooltip state belongs to the label, not the switch's data-state. */}
                <HoverTip content={collectionDescription} disableHoverableContent>
                  <label className="flex cursor-pointer items-center gap-2 whitespace-nowrap text-caption text-fg-2">
                    <span>{l("采集", "Collection")}</span>
                    <Switch
                      size="sm"
                      checked={applicationSettings(snapshot.applicationSettings, h.id).collectionEnabled}
                      disabled={savingApp !== null}
                      aria-label={`${h.name} ${l("采集", "collection")}`}
                      aria-describedby={`application-${h.id}-collection-help`}
                      onCheckedChange={(collectionEnabled) =>
                        void saveApp(h.id, { collectionEnabled })
                      }
                    />
                    <span id={`application-${h.id}-collection-help`} className="sr-only">
                      {collectionDescription}
                    </span>
                  </label>
                </HoverTip>
                <HoverTip content={visibilityDescription} disableHoverableContent>
                  <label className="flex cursor-pointer items-center gap-2 whitespace-nowrap text-caption text-fg-2">
                    <span>{l("显示", "Show")}</span>
                    <Switch
                      size="sm"
                      checked={applicationSettings(snapshot.applicationSettings, h.id).visible}
                      disabled={savingApp !== null}
                      aria-label={`${h.name} ${l("显示", "visibility")}`}
                      aria-describedby={`application-${h.id}-visibility-help`}
                      onCheckedChange={(visible) => void saveApp(h.id, { visible })}
                    />
                    <span id={`application-${h.id}-visibility-help`} className="sr-only">
                      {visibilityDescription}
                    </span>
                  </label>
                </HoverTip>
                <Button
                  variant="neutral"
                  size="compact"
                  onClick={() =>
                    setConfig((c) => ({
                      ...c,
                      sources: [
                        ...c.sources,
                        {
                          id: crypto.randomUUID(),
                          harness: h.id,
                          root: h.defaultRoots[0] || "",
                          enabled: false,
                        },
                      ],
                    }))
                  }
                >
                  <Plus size={14} />
                  {l("添加目录", "Add directory")}
                </Button>
              </div>
            </div>
            {h.id === "copliot-deprecated" && (
              <p className="mt-3 text-caption text-fg-2">
                {l(
                  "已选中的 Copilot 历史会话。按保存的用户请求计数，缺失 Token 按 0 统计；已知 Token 按模型价表估算参考费用，其他费用未知。新会话不会自动加入。",
                  "Selected historical Copilot sessions. Counts saved user requests and uses 0 for missing tokens. Known tokens use model reference pricing; other costs remain unknown. New sessions are not added automatically.",
                )}
              </p>
            )}
            {h.id === "copliot" && (
              <p className="mt-3 text-caption text-fg-2">
                {l(
                  "自动采集历史清单之外的新 Copilot 会话。目录需包含历史会话清单；按用户请求计数，仅累计输出 Token 接入统计，输入及缓存缺失填 0。已知 Token 可估算模型参考费，Credits 不换算为美元。",
                  "Collects new Copilot sessions outside the historical selection. The directory must contain that selection. Counts user requests and cumulative output tokens; unavailable input/cache usage uses 0. Known tokens may use model reference pricing; Credits are not converted to USD.",
                )}
              </p>
            )}
            {h.id === "cherry" && (
              <p className="mt-3 text-caption text-fg-2">
                {l(
                  "只采集能完整核对 Token 分类的逐次调用。未知的旧版汇总不计入；缺失分类的调用会在扫描诊断中提示。费用优先使用供应商上报的美元金额，否则按匹配价表估算。",
                  "Only individual calls with a complete token breakdown are counted. Unknown legacy aggregates are excluded; calls with missing breakdowns are reported in scan diagnostics. Provider-reported USD costs take priority; otherwise matching prices provide estimates.",
                )}
              </p>
            )}
            {h.id === "lmstudio" && (
              <p className="mt-3 text-caption text-fg-2">
                {l(
                  "只读本机桌面会话，统计所有生成版本；不采集服务器日志或 API 历史。输入为原始 prompt 总量，缓存与推理 Token 细分未提供，费用未计价。生成时间取自会话步骤标识，耗时与首字延迟取自生成统计并仅保留在本机。",
                  "Reads local desktop conversations and counts every generation version. Server logs and API history are excluded. Input is the original prompt total; cache/reasoning breakdowns are unreported and costs remain unpriced. Generation time comes from the step identity; reported duration and TTFT stay on this device.",
                )}
              </p>
            )}
            <div className="mt-5 space-y-3">
              {config.sources
                .filter((s) => s.harness === h.id)
                .map((s) => (
                  <div key={s.id} className="flex items-center gap-3">
                    <Switch
                      checked={s.enabled}
                      aria-label={`${h.name} ${l("目录采集", "directory collection")} ${s.root}`}
                      onCheckedChange={(enabled) =>
                        setConfig((c) => ({
                          ...c,
                          sources: c.sources.map((x) =>
                            x.id === s.id ? { ...x, enabled } : x,
                          ),
                        }))
                      }
                    />
                    <Input
                      aria-label={`${h.name} ${l("日志目录或数据库", "log directory or database")}`}
                      placeholder={h.defaultRoots[0] || ""}
                      value={s.root}
                      onChange={(e) =>
                        setConfig((c) => ({
                          ...c,
                          sources: c.sources.map((x) =>
                            x.id === s.id ? { ...x, root: e.target.value } : x,
                          ),
                        }))
                      }
                    />
                    {isTauri() && (
                      <Button
                        variant="ghost"
                        size="compact"
                        aria-label="Choose folder"
                        onClick={() => void browse(s.id)}
                      >
                        <Folder size={14} />
                      </Button>
                    )}
                    <Button
                      variant="ghost"
                      size="compact"
                      aria-label="Remove source"
                      onClick={() =>
                        setConfig((c) => ({
                          ...c,
                          sources: c.sources.filter((x) => x.id !== s.id),
                        }))
                      }
                    >
                      <Trash2 size={14} />
                    </Button>
                  </div>
                ))}
            </div>
          </section>
        ))}
        <section className="scope-card">
          <h2 className="font-semibold">
            {l("采集诊断", "Collection diagnostics")}
          </h2>
          <p className="mt-2 text-body text-fg-2">
            {snapshot.lastScan
              ? `${snapshot.lastScan.filesScanned} ${l("个文件", "files")} · ${snapshot.lastScan.deferredFiles} ${l("个待重试", "deferred")}`
              : l("尚未运行", "Not run yet")}
          </p>
          {snapshot.lastScan?.errors.map((error, i) => (
            <p key={i} className="mt-2 text-caption text-danger-text break-all">
              {error}
            </p>
          ))}
        </section>
      </div>
    </>
  );
}
