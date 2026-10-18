import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Folder, Save, Trash2 } from "lucide-react";
import { isTauri } from "@tauri-apps/api/core";
import type { Snapshot, Config } from "@/App";
import { command } from "@/lib/client";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { toast } from "@/lib/toast";
import { AppGlyph } from "@/components/shell/AppGlyph";
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
  const [config, setConfig] = useState<Config>(snapshot.config);
  const [busy, setBusy] = useState(false);
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
            {l("保存", "Save")}
          </Button>
        }
      />
      <div className="scope-page space-y-5">
        {snapshot.harnesses.map((h) => (
          <section key={h.id} className="scope-card">
            <div className="flex items-center gap-3">
              <div className="scope-app-icon">
                <AppGlyph app={h.id} size={24} badgeClassName="bg-surface" />
              </div>
              <div>
                <h2 className="font-semibold">{h.name}</h2>
                <p className="text-caption text-fg-3">
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
              <span className="flex-1" />
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
            <div className="mt-5 space-y-3">
              {config.sources
                .filter((s) => s.harness === h.id)
                .map((s) => (
                  <div key={s.id} className="flex items-center gap-3">
                    <Switch
                      checked={s.enabled}
                      aria-label={`${h.name} ${l("采集", "collection")}`}
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
