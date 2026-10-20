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
import { codexAllowancesApi as api } from "@/lib/api/codexAllowances";
import { useUnsavedChangesTracker } from "@/lib/unsavedChanges";
import { toast } from "@/lib/toast";
import type {
  CodexAllowanceCatalog,
  CodexAllowanceInfo,
} from "@/types/codexAllowances";
export function CodexAllowanceEditor({
  info,
  draft,
  zh,
  onClose,
}: {
  info: CodexAllowanceInfo;
  draft: CodexAllowanceCatalog;
  zh: boolean;
  onClose: () => void;
}) {
  const l = (a: string, b: string) => (zh ? a : b);
  const client = useQueryClient();
  const tracker = useUnsavedChangesTracker();
  const dirty = useRef(false);
  const [text, setText] = useState(JSON.stringify(draft, null, 2));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const close = () => {
    if (
      !busy &&
      (!dirty.current ||
        window.confirm(
          l("放弃未应用的配置修改？", "Discard unapplied changes?"),
        ))
    )
      onClose();
  };
  const save = async () => {
    setBusy(true);
    setError("");
    try {
      const next = await api.save(JSON.parse(text), info);
      client.setQueryData(["codex-allowances"], next);
      await client.invalidateQueries({
        queryKey: ["usage", "codex-reference"],
      });
      toast.success(
        l(
          "参考额度配置已应用，历史费用保持不变",
          "Allowance configuration applied; historical costs kept",
        ),
      );
      onClose();
    } catch (e) {
      setError(String(e));
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
      <SheetContent width={720} closeLabel={l("关闭", "Close")}>
        <SheetHeader>
          <SheetTitle>
            {l("OpenAI 本地参考额度配置", "OpenAI local allowance estimates")}
          </SheetTitle>
          <SheetDescription>
            {l(
              "每周 API 等值与订阅月费分别配置。缺少样本用 null，理论倍率推算会单独标记；只影响估算。",
              "Weekly API equivalents and monthly fees are separate. Use null for missing samples; theoretical projections are labeled. Affects estimates only.",
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
              aria-label={l("OpenAI 额度配置 JSON", "OpenAI allowance JSON")}
              value={text}
              disabled={busy}
              maxLength={262144}
              className="font-mono text-caption min-h-96 resize-y"
              onChange={(e) => {
                dirty.current = true;
                setText(e.target.value);
                setError("");
              }}
            />
            <p className="text-caption text-fg-3">
              {l(
                "apiEquivalentUsd 为该模型完整窗口的参考 API 等值（USD），不是月费；tokens 为绝对 Token 数，1 亿 = 100000000，包含缓存。文件是草稿，校验并应用后生效。",
                "apiEquivalentUsd is an estimated full-window USD equivalent, not the monthly fee. tokens is an absolute count including cache (100 million = 100000000). Files are drafts until validated and applied.",
              )}
            </p>
          </div>
        </SheetBody>
        <SheetFooter>
          <Button variant="neutral" disabled={busy} onClick={close}>
            {l("取消", "Cancel")}
          </Button>
          <Button disabled={busy} onClick={() => void save()}>
            {busy
              ? l("保存中…", "Saving…")
              : l("校验并应用", "Validate and apply")}
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}
