import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useIsMutating } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ImagePlus, LoaderCircle, RotateCcw, Trash2, X } from "lucide-react";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Button } from "@/components/ui/button";
import { HoverTip } from "@/components/ui/hover-tip";
import { cn } from "@/lib/utils";
import { toast } from "@/lib/toast";
import {
  ICON_PRESETS, ICON_QUERY_KEY, configuredIcon, iconUses, useIconLibrary, useIconMutation,
  type IconRef, type IconTarget,
} from "@/lib/entityIcons";
import { DEFAULT_ICON_CROP, normalizeIcon, prepareIcon, type IconCrop, type PreparedIcon } from "@/lib/iconUpload";
import { IconArtwork } from "./IconArtwork";

export function IconPickerPopover({ target, name, fallback, children, className }: {
  target: IconTarget; name: string; fallback?: IconRef; children: ReactNode; className?: string;
}) {
  const { i18n } = useTranslation();
  const l = (zh: string, en: string) => i18n.language.startsWith("zh") ? zh : en;
  const library = useIconLibrary();
  const mutation = useIconMutation();
  const saving = useIsMutating({ mutationKey: ICON_QUERY_KEY }) > 0;
  const [open, setOpen] = useState(false);
  const [showLibrary, setShowLibrary] = useState(false);
  const [presetGroup, setPresetGroup] = useState("brand");
  const [source, setSource] = useState<PreparedIcon>();
  const [crop, setCrop] = useState<IconCrop>(DEFAULT_ICON_CROP);
  const [processing, setProcessing] = useState(false);
  const [error, setError] = useState("");
  const fileInput = useRef<HTMLInputElement>(null);
  const generation = useRef(0);
  const label = l(`更换 ${name} 图标`, `Change ${name} icon`);
  const selected = configuredIcon(library.data, target);
  const assets = Object.values(library.data?.assets ?? {});
  useEffect(() => () => source?.dispose(), [source]);
  useEffect(() => () => { generation.current++; }, []);
  const preview = useMemo(() => {
    if (!source) return undefined;
    try { return { ...normalizeIcon(source, crop), error: "" }; }
    catch (e) { return { dataUrl: "", size: 0, error: String(e) }; }
  }, [source, crop]);
  const clearDraft = () => {
    generation.current++;
    setSource(undefined);
    setProcessing(false);
    setError("");
    if (fileInput.current) fileInput.current.value = "";
  };
  const changeOpen = (next: boolean) => {
    setOpen(next);
    if (!next) clearDraft();
    else {
      setError("");
      setPresetGroup(ICON_PRESETS.find(p => `builtin:${p.id}` === (selected ?? fallback))?.group ?? "brand");
      if (!saving) void library.refetch();
    }
  };
  const choose = async (icon: IconRef | null) => {
    setError("");
    try {
      await mutation.mutateAsync({ command: "set_entity_icon", args: { target, icon } });
      changeOpen(false);
      toast.success(l("图标已保存", "Icon saved"));
    } catch (e) { setError(String(e)); }
  };
  const upload = async (file: File | undefined) => {
    if (!file) return;
    clearDraft();
    const current = ++generation.current;
    setProcessing(true);
    try {
      const prepared = await prepareIcon(file);
      if (current !== generation.current) { prepared.dispose(); return; }
      setCrop(DEFAULT_ICON_CROP);
      setSource(prepared);
    } catch (e) { if (current === generation.current) setError(String(e)); }
    finally { if (current === generation.current) setProcessing(false); }
  };
  const apply = async () => {
    if (!source || !preview || preview.error) return;
    setError("");
    try {
      await mutation.mutateAsync({ command: "import_entity_icon", args: { target, name: source.name, dataUrl: preview.dataUrl } });
      changeOpen(false);
      toast.success(l("图标已保存", "Icon saved"));
    } catch (e) { setError(String(e)); }
  };
  const remove = async (id: string) => {
    setError("");
    try { await mutation.mutateAsync({ command: "delete_icon_asset", args: { id } }); }
    catch (e) { setError(String(e)); void library.refetch(); }
  };
  const disabled = saving || processing || !library.data || library.isError;
  return <Popover open={open} onOpenChange={changeOpen}>
    <HoverTip content={open ? undefined : label} disableHoverableContent>
      <PopoverTrigger asChild>
        <button type="button" aria-label={label} className={cn(
          "shrink-0 rounded-control hover:bg-subtle focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
          className,
        )}>{children}</button>
      </PopoverTrigger>
    </HoverTip>
    <PopoverContent align="start" sideOffset={8} collisionPadding={12} aria-label={label}
      className="w-[340px] max-w-[calc(100vw-24px)] max-h-[var(--radix-popover-content-available-height)] overflow-y-auto rounded-xl p-3">
      <div className="mb-3 flex items-center gap-2">
        <span className="min-w-0 flex-1 break-words text-body font-medium">{source ? l("裁剪图标", "Crop icon") : label}</span>
        <Button type="button" variant="ghost" size="icon-compact" aria-label={l("关闭图标选择", "Close icon picker")} onClick={() => changeOpen(false)}><X size={14} /></Button>
      </div>
      <input ref={fileInput} className="hidden" type="file" accept=".svg,.png,.jpg,.jpeg,.webp" aria-label={l("上传图标文件", "Upload icon file")}
        onChange={e => void upload(e.target.files?.[0])} />
      {library.isError && <div role="alert" className="mb-3 text-caption text-danger-text">
        {l("图标配置读取失败", "Could not load icons")}
        <Button type="button" variant="ghost" size="compact" onClick={() => void library.refetch()}>{l("重试", "Retry")}</Button>
      </div>}
      {!library.data && !library.isError && <p role="status" className="mb-3 text-caption text-fg-3">{l("正在读取图标…", "Loading icons…")}</p>}
      {source ? <div className="space-y-3">
        <p className="break-all text-caption text-fg-2">{source.name} · {source.width} × {source.height}</p>
        <div className="flex flex-wrap items-center gap-2">
          {(["crop", "fit"] as const).map(mode => <Button key={mode} type="button" variant={crop.mode === mode ? "neutral" : "ghost"} size="compact"
            aria-pressed={crop.mode === mode} disabled={saving} onClick={() => setCrop(c => ({ ...c, mode }))}>
            {mode === "crop" ? l("裁剪为方形", "Square crop") : l("保留完整图片", "Fit entire image")}
          </Button>)}
        </div>
        {preview && !preview.error && <>
          <div className="grid grid-cols-2 gap-3">
            {(["light", "dark"] as const).map(theme => <div key={theme} className="space-y-1">
              <div className={cn("flex aspect-square items-center justify-center rounded-lg border border-border p-3", theme === "light" ? "bg-white" : "bg-[#171b25]")}>
                <img src={preview.dataUrl} alt={l(`${theme === "light" ? "浅" : "深"}色背景裁剪预览`, `${theme} background crop preview`)}
                  className="max-h-32 max-w-full object-contain" width={preview.size} height={preview.size} />
              </div>
              <p className="text-center text-caption text-fg-3">{theme === "light" ? l("浅色背景", "Light background") : l("深色背景", "Dark background")}</p>
            </div>)}
          </div>
          <p className="text-caption text-fg-2" role="status">{l("输出", "Output")} {preview.size} × {preview.size} · {source.svg ? "SVG" : "PNG"}</p>
        </>}
        {crop.mode === "crop" && <div className="space-y-2">
          {([
            { key: "zoom", text: l("缩放", "Zoom"), min: 1, max: 4, step: 0.05 },
            { key: "x", text: l("水平位置", "Horizontal position"), min: 0, max: 100, step: 1 },
            { key: "y", text: l("垂直位置", "Vertical position"), min: 0, max: 100, step: 1 },
          ] as const).map(field => <label key={field.key} className="flex items-center gap-3 text-caption text-fg-2">
            <span className="w-20 shrink-0">{field.text}</span>
            <input type="range" className="min-w-0 flex-1 accent-action" aria-label={field.text} disabled={saving}
              min={field.min} max={field.max} step={field.step} value={crop[field.key]}
              onChange={e => setCrop(c => ({ ...c, [field.key]: Number(e.target.value) }))} />
          </label>)}
        </div>}
        <div className="flex justify-end gap-2">
          <Button type="button" variant="ghost" size="compact" disabled={saving} onClick={clearDraft}>{l("取消", "Cancel")}</Button>
          <Button type="button" size="compact" disabled={disabled || !preview || !!preview.error} onClick={() => void apply()}>{l("应用图标", "Apply icon")}</Button>
        </div>
      </div> : <>
        <div role="group" aria-label={l("预置图标风格", "Preset icon style")} className="mb-3 grid grid-cols-3 gap-1 rounded-lg bg-subtle p-1">
          {[
            { id: "brand", label: l("品牌图标", "Brand") },
            { id: "portrait", label: l("社区头像", "Community") },
            { id: "pixel", label: l("像素头像", "Pixel") },
          ].map(group => <Button key={group.id} type="button" size="compact"
            variant={presetGroup === group.id ? "neutral" : "ghost"} aria-pressed={presetGroup === group.id}
            onClick={() => setPresetGroup(group.id)}>{group.label}</Button>)}
        </div>
        <div className="grid grid-cols-6 gap-2" aria-label={l("预置图标", "Preset icons")}>
          {ICON_PRESETS.filter(preset => preset.group === presetGroup).map(preset => <HoverTip key={preset.id} content={preset.name}>
            <button type="button" aria-label={l(`使用 ${preset.name} 图标`, `Use ${preset.name} icon`)}
              aria-pressed={(selected ?? fallback) === `builtin:${preset.id}`} disabled={disabled}
              onClick={() => void choose(`builtin:${preset.id}`)}
              className={cn("relative flex aspect-square min-w-0 items-center justify-center overflow-hidden rounded-lg border border-border p-0 text-fg-1 hover:bg-subtle disabled:opacity-50 focus-visible:ring-2 focus-visible:ring-ring",
                (selected ?? fallback) === `builtin:${preset.id}` && "border-action bg-subtle ring-1 ring-ring")}>
              <IconArtwork icon={`builtin:${preset.id}`} size={24} fill={presetGroup !== "brand"} />
            </button>
          </HoverTip>)}
          <HoverTip content={l("上传 SVG 或图片", "Upload SVG or image")}>
            <button type="button" aria-label={l("上传 SVG 或图片", "Upload SVG or image")} disabled={disabled}
              className="flex aspect-square items-center justify-center rounded-lg border border-dashed border-border text-fg-2 hover:bg-subtle disabled:opacity-50"
              onClick={() => fileInput.current?.click()}><ImagePlus size={20} /></button>
          </HoverTip>
        </div>
        <p className="mt-2 text-caption text-fg-3">SVG / PNG / JPG / WebP · ≤ 1 MB · {l("裁剪至", "Resize to")} ≤ 128 × 128</p>
        {showLibrary && <div className="mt-3 space-y-2" aria-label={l("我的图标", "My icons")}>
          {assets.length === 0 && <p className="py-2 text-caption text-fg-3">{l("暂无上传图标", "No uploaded icons")}</p>}
          {assets.map(asset => {
            const uses = iconUses(library.data!, asset.id);
            return <div key={asset.id} className="flex items-center gap-2">
              <button type="button" disabled={disabled} onClick={() => void choose(`asset:${asset.id}`)}
                aria-label={l(`使用 ${asset.name}`, `Use ${asset.name}`)} aria-pressed={selected === `asset:${asset.id}`}
                className={cn("flex min-w-0 flex-1 items-center gap-2 rounded-lg p-1.5 text-start hover:bg-subtle disabled:opacity-50", selected === `asset:${asset.id}` && "bg-subtle ring-1 ring-ring")}>
                <IconArtwork icon={`asset:${asset.id}`} library={library.data} size={28} />
                <span className="min-w-0 flex-1"><span className="block break-all text-caption">{asset.name}</span>
                  <span className="block text-caption text-fg-3">{uses ? l(`${uses} 处使用中`, `Used in ${uses} places`) : l("未使用", "Unused")}</span></span>
              </button>
              <HoverTip content={uses ? l("使用中的图标不可删除，请先更换或恢复默认", "Replace or reset this icon before deleting it") : l("删除图标", "Delete icon")}>
                <Button type="button" variant="ghost" size="icon-compact" disabled={disabled || uses > 0}
                  aria-label={l(`删除 ${asset.name}`, `Delete ${asset.name}`)} onClick={() => void remove(asset.id)}><Trash2 size={14} /></Button>
              </HoverTip>
            </div>;
          })}
        </div>}
        <div className="mt-3 flex items-center justify-between gap-2 border-t border-border pt-2">
          <Button type="button" variant="ghost" size="compact" aria-expanded={showLibrary} onClick={() => setShowLibrary(v => !v)}>{l("我的图标", "My icons")} {assets.length}</Button>
          <Button type="button" variant="ghost" size="compact" disabled={disabled || !selected} onClick={() => void choose(null)}><RotateCcw size={12} />{l("恢复默认", "Reset")}</Button>
        </div>
      </>}
      {(error || preview?.error) && <p role="alert" className="mt-3 break-words text-caption text-danger-text">{error || preview?.error}</p>}
      {(saving || processing) && <p role="status" className="mt-2 flex items-center gap-1 text-caption text-fg-2"><LoaderCircle size={12} className="animate-spin" />{processing ? l("正在处理图片…", "Processing image…") : l("保存中…", "Saving…")}</p>}
      <p className="mt-2 break-words text-caption text-fg-3">{target.kind === "account" ? l(`仅当前账号：${name}`, `This account only: ${name}`) : l(`应用：${name}`, `Application: ${name}`)} · {l("本机保存", "Saved on this device")}</p>
    </PopoverContent>
  </Popover>;
}
