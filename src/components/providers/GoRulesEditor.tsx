import { useRef, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import {
  Sheet,
  SheetBody,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { goCatalogApi } from "@/lib/api/goCatalog";
import { goError } from "@/lib/goCatalog";
import { useUnsavedChangesTracker } from "@/lib/unsavedChanges";
import { toast } from "@/lib/toast";
import type { GoCatalog, GoCatalogInfo } from "@/types/goCatalog";
export function GoRulesEditor({
  info,
  draft,
  zh,
  onClose,
}: {
  info: GoCatalogInfo;
  draft: GoCatalog;
  zh: boolean;
  onClose: () => void;
}) {
  const l = (a: string, b: string) => (zh ? a : b);
  const client = useQueryClient();
  const [text, setText] = useState(JSON.stringify(draft, null, 2));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const dirty = useRef(false);
  const tracker = useUnsavedChangesTracker();
  const close = () => {
    if (
      !busy &&
      (!dirty.current ||
        window.confirm(
          l("放弃未应用的规则修改？", "Discard unapplied rule changes?"),
        ))
    )
      onClose();
  };
  const parsed = (() => {
    try {
      return JSON.parse(text) as GoCatalog;
    } catch {
      return null;
    }
  })();
  const save = async () => {
    setBusy(true);
    setError("");
    try {
      const catalog = JSON.parse(text) as GoCatalog;
      const next = await goCatalogApi.save(catalog, info);
      client.setQueryData(["go-catalog"], next);
      await Promise.all([
        client.invalidateQueries({ queryKey: ["usage", "go-usage"] }),
        client.invalidateQueries({ queryKey: ["usage", "pricing"] }),
      ]);
      toast.success(
        l(
          "Go 规则已应用，已有费用快照保留",
          "Go rules applied; existing cost snapshots kept",
        ),
      );
      onClose();
    } catch (e) {
      setError(goError(e, zh));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Sheet
      open
      onOpenChange={(open) => {
        if (!open) close();
      }}
    >
      <SheetContent width={680} closeLabel={l("关闭", "Close")}>
        <SheetHeader>
          <SheetTitle>
            {l("OpenCode Go 本地规则", "OpenCode Go local rules")}
          </SheetTitle>
          <SheetDescription>
            {l(
              "预览并校验 JSON 后应用。只影响后续计价与当前表的用量估算，已有费用不自动重算。",
              "Preview and validate JSON before applying. Affects subsequent pricing and allowance estimates; historical costs are not automatically recalculated.",
            )}
          </SheetDescription>
        </SheetHeader>
        <SheetBody>
          <div className="space-y-3" {...tracker}>
            <p className="text-caption text-fg-3 break-all">{info.path}</p>
            {error && (
              <p
                role="alert"
                className="rounded-control bg-danger-soft p-3 text-caption text-danger-text"
              >
                {error}
              </p>
            )}
            <Textarea
              aria-label={l("Go 规则 JSON", "Go rules JSON")}
              value={text}
              disabled={busy}
              maxLength={262144}
              className="font-mono text-caption min-h-80 resize-y"
              onChange={(e) => {
                dirty.current = true;
                setText(e.target.value);
                setError("");
              }}
            />
            <p className="text-caption text-fg-3">
              {l(
                "修改单位为 USD / 百万 Tokens；null 表示未提供，不能当作零价。JSON 合法不等于官方内容已核实，请对照来源。",
                "Rates are USD per million tokens; null means unprovided, not zero. Valid JSON still needs verification against the official source.",
              )}
            </p>
            {Array.isArray(parsed?.models) && (
              <section className="border border-border rounded-control p-3 text-caption">
                {l("待应用版本", "Pending version")}:{" "}
                {String(parsed.version ?? "—")} · {parsed.models.length}{" "}
                {l("模型", "models")}
              </section>
            )}
          </div>
        </SheetBody>
        <SheetFooter>
          <Button variant="neutral" disabled={busy} onClick={close}>
            {l("取消", "Cancel")}
          </Button>
          <Button disabled={busy || !parsed} onClick={() => void save()}>
            {busy
              ? l("保存中…", "Saving…")
              : l("校验并应用", "Validate and apply")}
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}
