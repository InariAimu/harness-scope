import { OptionSelect } from "@/components/ui/option-select";
import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { isTauri } from "@tauri-apps/api/core";
import {
  Save,
  Cloud,
  Database,
  SlidersHorizontal,
  Info,
  RefreshCw,
} from "lucide-react";
import type { Snapshot, Config } from "@/App";
import { command } from "@/lib/client";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { DatePicker } from "@/components/ui/date-picker";
import { AccentColorSettings } from "@/components/settings/AccentColorSettings";
import { useAccentThemePreview } from "@/components/theme/AccentThemeProvider";
import { DEFAULT_ACCENT_COLOR, normalizeAccentColor } from "@/lib/accentColor";
import { INTERFACE_SKINS, normalizeInterfaceSkin } from "@/lib/interfaceSkins";
import {
  DEFAULT_SKIN_TRANSPARENCY,
  DEFAULT_SKIN_MASK_STRENGTH,
  skinPercentage,
} from "@/lib/skinAppearance";
import { toast } from "@/lib/toast";
function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    <label className="scope-field">
      <span>{label}</span>
      {children}
    </label>
  );
}
export function SettingsPage({
  snapshot,
  onChanged,
}: {
  snapshot: Snapshot;
  onChanged: () => unknown;
}) {
  const { i18n } = useTranslation();
  const l = (zh: string, en: string) =>
    i18n.language.startsWith("zh") ? zh : en;
  const [config, setConfig] = useState<Config>(() => ({
    ...snapshot.config,
    accentColor: normalizeAccentColor(snapshot.config.accentColor) ?? DEFAULT_ACCENT_COLOR,
    interfaceSkin: normalizeInterfaceSkin(snapshot.config.interfaceSkin),
    skinTransparency: skinPercentage(snapshot.config.skinTransparency, DEFAULT_SKIN_TRANSPARENCY),
    skinMaskStrength: skinPercentage(snapshot.config.skinMaskStrength, DEFAULT_SKIN_MASK_STRENGTH),
  }));
  const [accentValid, setAccentValid] = useState(true);
  const setThemePreview = useAccentThemePreview();
  useEffect(() => {
    setThemePreview({
      accentColor: config.accentColor,
      theme: config.theme,
      interfaceSkin: config.interfaceSkin,
      skinTransparency: config.skinTransparency,
      skinMaskStrength: config.skinMaskStrength,
    });
  }, [config.accentColor, config.theme, config.interfaceSkin, config.skinTransparency, config.skinMaskStrength, setThemePreview]);
  useEffect(() => () => setThemePreview(null), [setThemePreview]);
  const [section, setSection] = useState("general");
  const [busy, setBusy] = useState(false);
  const [migration, setMigration] = useState({
    path: "",
    sourceId: "ccswitch-local",
    cutoverDate: new Date().toLocaleDateString("en-CA"),
    timezone: snapshot.config.timezone,
  });
  const [preview, setPreview] = useState<Record<string, unknown> | null>(null);
  const update = <K extends keyof Config>(key: K, value: Config[K]) =>
    setConfig((c) => ({ ...c, [key]: value }));
  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await fn();
      await onChanged();
    } catch (e) {
      toast.error(String(e));
    } finally {
      setBusy(false);
    }
  };
  const save = () =>
    run(async () => {
      await command("save_config", { config });
      toast.success(l("设置已保存", "Settings saved"));
    });
  const migrate = async (commit = false) =>
    run(async () => {
      const value = await command<Record<string, unknown>>(
        commit ? "migration_commit" : "migration_preview",
        { ...migration, expectedDigest: commit ? preview?.digest : null },
      );
      setPreview(value);
      toast.success(
        commit
          ? l("历史数据迁移完成", "History imported")
          : l("快照已检查，请核对预览", "Snapshot checked; review the preview"),
      );
    });
  const browse = async () => {
    if (!isTauri()) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const path = await open({ multiple: false });
    if (typeof path === "string") {
      setMigration((m) => ({ ...m, path }));
      setPreview(null);
    }
  };
  return (
    <>
      <AppPageHeader
        title={l("设置", "Settings")}
        actions={
          <Button onClick={() => void save()} disabled={busy || !accentValid}>
            <Save size={14} />
            {l("保存设置", "Save settings")}
          </Button>
        }
      />
      <div className="scope-page flex flex-col gap-4 md:flex-row md:gap-6">
        <nav className="scope-settings-nav flex flex-wrap gap-1 md:block md:space-y-1">
          {[
            {
              id: "general",
              name: l("通用", "General"),
              Icon: SlidersHorizontal,
            },
            { id: "sync", name: l("多设备同步", "Device sync"), Icon: Cloud },
            {
              id: "data",
              name: l("数据迁移", "Data migration"),
              Icon: Database,
            },
            { id: "about", name: l("关于", "About"), Icon: Info },
          ].map(({ id, name, Icon }) => (
            <button
              key={id}
              onClick={() => setSection(id)}
              className={`scope-nav ${section === id ? "active" : ""}`}
            >
              <Icon size={15} />
              {name}
            </button>
          ))}
        </nav>
        <div className="min-w-0 flex-1 max-w-3xl">
          {section === "general" && (
            <section className="scope-card">
              <h2 className="font-semibold mb-5">{l("通用", "General")}</h2>
              <Field label={l("设备名称", "Device name")}>
                <Input
                  value={config.deviceName}
                  onChange={(e) => update("deviceName", e.target.value)}
                />
              </Field>
              <Field label={l("主题", "Theme")}>
                <OptionSelect
                  className="scope-select"
                  value={config.theme}
                  onValueChange={(value) => update("theme", value)}
                >
                  <option value="system">{l("跟随系统", "System")}</option>
                  <option value="light">{l("浅色", "Light")}</option>
                  <option value="dark">{l("深色", "Dark")}</option>
                </OptionSelect>
              </Field>
              <Field label={l("界面皮肤", "Interface skin")}>
                <OptionSelect
                  aria-label={l("界面皮肤", "Interface skin")}
                  value={config.interfaceSkin}
                  onValueChange={(value) => update("interfaceSkin", normalizeInterfaceSkin(value))}
                >
                  <option value="none">{l("无 · 纯色界面", "None · Solid background")}</option>
                  {INTERFACE_SKINS.map((skin) => (
                    <option key={skin.id} value={skin.id}>{l(skin.name.zh, skin.name.en)}</option>
                  ))}
                </OptionSelect>
                <p className="text-caption text-fg-3">
                  {l(
                    "右侧角色搭配半透明遮罩，保留内容阅读空间。皮肤和重点色可分别选择，保存设置后保留。",
                    "Right-side artwork with a translucent veil keeps content readable. Choose skin and accent independently, then save to keep them.",
                  )}
                </p>
              </Field>
              <fieldset
                disabled={config.interfaceSkin === "none"}
                className="mb-5 space-y-3 disabled:opacity-45"
                aria-label={l("皮肤显示调节", "Skin appearance")}
              >
                {([
                  ["skinTransparency", l("皮肤透明度", "Skin transparency"), l("越高，角色越淡；100% 完全透明。", "Higher values fade the character; 100% is fully transparent.")],
                  ["skinMaskStrength", l("遮罩强度", "Veil strength"), l("越高，背景遮罩越实，内容越清晰。", "Higher values strengthen the veil for clearer content.")],
                ] as const).map(([key, label, hint]) => (
                  <div key={key} className="space-y-1">
                    <div className="flex items-center justify-between gap-3 text-caption">
                      <label htmlFor={key} className="text-fg-2">{label}</label>
                      <output htmlFor={key} className="font-mono tabular-nums">{config[key]}%</output>
                    </div>
                    <input
                      id={key}
                      type="range"
                      min={0}
                      max={100}
                      step={1}
                      value={config[key]}
                      aria-valuetext={`${config[key]}%`}
                      aria-describedby={`${key}-hint`}
                      onChange={(event) => update(key, Number(event.target.value))}
                      className="block h-6 w-full cursor-pointer rounded-control focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed"
                      style={{ accentColor: "var(--action-bg)" }}
                    />
                    <p id={`${key}-hint`} className="text-caption text-fg-3">{hint}</p>
                  </div>
                ))}
                <Button
                  type="button"
                  variant="neutral"
                  onClick={() => setConfig((current) => ({
                    ...current,
                    skinTransparency: DEFAULT_SKIN_TRANSPARENCY,
                    skinMaskStrength: DEFAULT_SKIN_MASK_STRENGTH,
                  }))}
                >
                  {l("恢复皮肤默认", "Reset skin appearance")}
                </Button>
                <p className="text-caption text-fg-3">
                  {l(
                    "拖动即时预览，保存设置后保留；未保存离开将恢复原设置。请先选择皮肤以启用调节。",
                    "Drag to preview, then save to keep changes. Leaving without saving restores saved values. Select a skin to enable these controls.",
                  )}
                </p>
              </fieldset>
              <AccentColorSettings
                value={config.accentColor}
                onChange={(color) => update("accentColor", color)}
                onValidityChange={setAccentValid}
              />
              <Field label={l("语言", "Language")}>
                <OptionSelect
                  className="scope-select"
                  value={config.language}
                  onValueChange={(value) => update("language", value)}
                >
                  <option value="zh">简体中文</option>
                  <option value="en">English</option>
                </OptionSelect>
              </Field>
              <Field
                label={l("统计时区（IANA）", "Statistics timezone (IANA)")}
              >
                <Input
                  value={config.timezone}
                  onChange={(e) => update("timezone", e.target.value)}
                />
              </Field>
              <Field
                label={l(
                  "后台采集间隔（秒）",
                  "Background collection interval (seconds)",
                )}
              >
                <Input
                  type="number"
                  min={10}
                  max={86400}
                  value={config.scanIntervalSeconds}
                  onChange={(e) =>
                    update("scanIntervalSeconds", Number(e.target.value))
                  }
                />
              </Field>
              <p className="text-caption text-fg-3">{snapshot.deviceId}</p>
            </section>
          )}
          {section === "sync" && (
            <section className="scope-card">
              <h2 className="font-semibold mb-5">
                {l("自部署服务器", "Self-hosted server")}
              </h2>
              <Field label={l("服务器地址", "Server URL")}>
                <Input
                  placeholder="https://usage.example.com"
                  value={config.serverUrl}
                  onChange={(e) => update("serverUrl", e.target.value)}
                />
              </Field>
              <Field label={l("访问令牌", "Access token")}>
                <Input
                  type="password"
                  autoComplete="off"
                  placeholder={
                    snapshot.tokenConfigured
                      ? l("已配置，留空保留", "Configured; leave blank to keep")
                      : l("输入服务器令牌", "Enter server token")
                  }
                  value={config.serverToken}
                  onChange={(e) => update("serverToken", e.target.value)}
                />
              </Field>
              <p className="text-body text-fg-2 mb-4">
                {l(
                  "仅同步用量、费用来源和设备标识。会话正文、项目路径和工具内容留在本机。",
                  "Only usage, cost provenance and device identities are synchronized. Transcripts, project paths and tool content stay local.",
                )}
              </p>
              <div className="flex gap-2">
                <Button
                  variant="neutral"
                  disabled={busy}
                  onClick={() =>
                    void run(async () => {
                      await command("save_config", { config });
                      const result = await command<{
                        uploaded: number;
                        downloaded: number;
                      }>("sync");
                      toast.success(
                        `${result.uploaded} ↑ · ${result.downloaded} ↓`,
                      );
                    })
                  }
                >
                  <RefreshCw size={14} />
                  {l("保存并同步", "Save and sync")}
                </Button>
              </div>
              <p className="mt-4 text-caption text-fg-3">
                {snapshot.pending} {l("条待上传", "pending records")} ·{" "}
                {snapshot.lastSyncAt
                  ? new Date(snapshot.lastSyncAt).toLocaleString()
                  : l("尚未同步", "Not synchronized")}
              </p>
              {snapshot.lastSyncError && (
                <p className="mt-2 text-caption text-danger-text break-all">
                  {snapshot.lastSyncError}
                </p>
              )}
            </section>
          )}
          {section === "data" && (
            <section className="scope-card">
              <h2 className="font-semibold mb-3">CC Switch → Harness Scope</h2>
              <p className="text-body text-fg-2 mb-5">
                {l(
                  "读取 schema v20 SQLite 一致性快照。分界日前导入旧统计，分界日及之后读取原生日志；已有旧汇总的日期无法恢复请求明细。",
                  "Reads a consistent schema v20 SQLite snapshot. Import historical totals before the cutover; collect native usage on and after it. Daily rollups cannot recover request details.",
                )}
              </p>
              <Field label={l("数据库或备份路径", "Database or backup path")}>
                <div className="flex gap-2">
                  <Input
                    value={migration.path}
                    onChange={(e) => {
                      setMigration((m) => ({ ...m, path: e.target.value }));
                      setPreview(null);
                    }}
                  />
                  {isTauri() && (
                    <Button variant="neutral" onClick={() => void browse()}>
                      {l("选择", "Choose")}
                    </Button>
                  )}
                </div>
              </Field>
              <Field
                label={l(
                  "旧来源身份（重复导入时保持相同）",
                  "Legacy source ID (keep stable)",
                )}
              >
                <Input
                  value={migration.sourceId}
                  onChange={(e) => {
                    setMigration((m) => ({ ...m, sourceId: e.target.value }));
                    setPreview(null);
                  }}
                />
              </Field>
              <div className="grid grid-cols-2 gap-4">
                <Field label={l("分界自然日", "Cutover date")}>
                  <DatePicker
                    aria-label={l("分界自然日", "Cutover date")}
                    value={migration.cutoverDate}
                    onValueChange={(value) => {
                      setMigration((m) => ({
                        ...m,
                        cutoverDate: value,
                      }));
                      setPreview(null);
                    }}
                  />
                </Field>
                <Field label={l("旧系统统计时区", "Legacy timezone")}>
                  <Input
                    value={migration.timezone}
                    onChange={(e) => {
                      setMigration((m) => ({ ...m, timezone: e.target.value }));
                      setPreview(null);
                    }}
                  />
                </Field>
              </div>
              <div className="flex gap-2">
                <Button
                  variant="neutral"
                  disabled={busy || !migration.path}
                  onClick={() => void migrate()}
                >
                  {l("预览与对账", "Preview")}
                </Button>
                <Button
                  disabled={busy || !preview}
                  onClick={() => void migrate(true)}
                >
                  {l("提交迁移", "Import snapshot")}
                </Button>
              </div>
              {preview && (
                <pre className="mt-5 rounded-panel bg-subtle p-4 overflow-auto text-caption">
                  {JSON.stringify(preview, null, 2)}
                </pre>
              )}
              <p className="mt-4 text-caption text-fg-3">
                {l("迁移记录", "Migration manifests")}:{" "}
                {snapshot.migrations.length}
              </p>
            </section>
          )}
          {section === "about" && (
            <section className="scope-card">
              <h2 className="text-lg font-semibold">Harness Scope</h2>
              <p className="mt-2 text-fg-2">v0.1.0 · MIT</p>
              <p className="mt-4">
                {l(
                  "模块化的本机用量统计与会话浏览器。",
                  "Modular local usage statistics and session browser.",
                )}
              </p>
              <p className="mt-4 text-caption text-fg-3">
                {l(
                  "界面组件和计价规则参考 CC Switch e8880e6。原作者 Jason Young；许可证与来源记录见 third_party。",
                  "UI components and pricing rules adapted from CC Switch e8880e6, Copyright Jason Young. See third_party for license and provenance.",
                )}
              </p>
            </section>
          )}
        </div>
      </div>
    </>
  );
}
