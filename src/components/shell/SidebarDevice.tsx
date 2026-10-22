import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Monitor, Pencil } from "lucide-react";
import type { Snapshot } from "@/App";
import { command } from "@/lib/client";
import { APP_BUILD, buildDescription } from "@/lib/appBuild";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Dialog, DialogContent, DialogDescription, DialogFooter,
  DialogHeader, DialogTitle, DialogTrigger,
} from "@/components/ui/dialog";

export function SidebarDevice({ snapshot, collapsed, zh }: {
  snapshot: Snapshot;
  collapsed: boolean;
  zh: boolean;
}) {
  const l = (a: string, b: string) => zh ? a : b;
  const client = useQueryClient();
  const inputId = useId();
  const input = useRef<HTMLInputElement>(null);
  const [open, setOpen] = useState(false);
  const [name, setName] = useState(snapshot.config.deviceName);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    if (error && !busy) input.current?.focus();
  }, [error, busy]);
  const trimmed = name.trim();
  const tooLong = new TextEncoder().encode(trimmed).length > 128;
  const invalid = !trimmed || tooLong || /[\u0000-\u001f\u007f-\u009f]/.test(trimmed);
  const displayName = snapshot.config.deviceName || l("本机", "This device");
  const build = snapshot.appBuild ?? APP_BUILD;
  const save = async (event: FormEvent) => {
    event.preventDefault();
    if (busy || invalid) return;
    setBusy(true);
    setError("");
    try {
      const saved = await command<string>("rename_device", { name: trimmed });
      client.setQueryData<Snapshot>(["snapshot"], (current) => current
        ? { ...current, config: { ...current.config, deviceName: saved } }
        : current);
      void client.invalidateQueries({ queryKey: ["snapshot"] });
      setOpen(false);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog open={open} onOpenChange={(next) => {
      if (busy) return;
      if (next) { setName(snapshot.config.deviceName); setError(""); }
      setOpen(next);
    }}>
      <DialogTrigger asChild>
        <button
          type="button"
          className={collapsed ? "scope-nav" : "flex w-full min-w-0 items-center gap-2 rounded-control px-3 pt-3 pb-1 text-caption text-fg-2 hover:bg-subtle focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"}
          aria-label={`${l("重命名此终端", "Rename this device")} · ${displayName}`}
          title={`${displayName}\nID: ${snapshot.deviceId}`}
        >
          {collapsed ? <Monitor size={16} aria-hidden="true" /> : <>
            <span className="min-w-0 truncate">{displayName}</span>
            <Pencil size={12} className="shrink-0 text-fg-3" aria-hidden="true" />
          </>}
        </button>
      </DialogTrigger>
      {!collapsed && <div className="flex min-w-0 gap-1 px-3 pb-2 text-caption text-fg-3">
        <span className="truncate" title={buildDescription(build, zh)}>v{build.version}{build.channel === "dev" ? "-dev" : ""}</span>
        <span className="shrink-0" title={`ID: ${snapshot.deviceId}`}>· ID {snapshot.deviceId.slice(0, 8)}</span>
      </div>}
      <DialogContent className="max-w-md">
        <form onSubmit={(event) => void save(event)}>
          <DialogHeader>
            <DialogTitle>{l("重命名此终端", "Rename this device")}</DialogTitle>
            <DialogDescription>{l(
              "名称用于识别终端，下次同步后其他终端也会看到新名称。终端 ID 保持不变。",
              "The name identifies this device and appears on other devices after syncing. Its ID stays the same.",
            )}</DialogDescription>
          </DialogHeader>
          <div className="px-6 space-y-3">
            <label className="block space-y-1" htmlFor={inputId}>
              <span className="text-caption text-fg-2">{l("自定义名称", "Custom name")}</span>
              <Input ref={input} id={inputId} value={name} disabled={busy} autoComplete="off"
                onChange={(event) => { setName(event.target.value); setError(""); }}
                aria-invalid={invalid} aria-describedby={tooLong || error ? `${inputId}-error` : undefined} />
            </label>
            <div className="text-caption text-fg-3">
              <p>{l("终端 ID", "Device ID")}</p>
              <p className="font-mono break-all select-text">{snapshot.deviceId}</p>
            </div>
            {(tooLong || error) && <p id={`${inputId}-error`} role="alert" className="text-caption text-danger-text">
              {tooLong ? l("名称过长，请缩短后保存。", "Name is too long. Shorten it before saving.") : error}
            </p>}
          </div>
          <DialogFooter>
            <Button type="button" variant="quiet" disabled={busy} onClick={() => setOpen(false)}>{l("取消", "Cancel")}</Button>
            <Button type="submit" disabled={busy || invalid}>{busy ? l("保存中…", "Saving…") : l("保存", "Save")}</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
