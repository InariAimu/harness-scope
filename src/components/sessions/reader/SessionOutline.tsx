import { memo, useEffect, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useVirtualizer } from "@tanstack/react-virtual";

import { cn } from "@/lib/utils";
import { formatMessageTime } from "../utils";
import { useReaderT } from "./i18n";
import { SessionAgentAvatar } from "./SessionAgentAvatar";
import { isFailureStep } from "./toolSummary";
import type { SessionTurn } from "./turns";

/** 目录里每条最多显示的字数：再多也只是两行截断，没必要整段传给 DOM */
const SNIPPET_CHARS = 160;

/** 去掉 Markdown 记号，只留可读文字作目录摘要 */
const toSnippet = (text: string) =>
  text
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, " ")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    // 只去掉 Markdown 记号；_ 和 - 常在标识符里（tokio_util、foo-bar），保留
    .replace(/[#>*`~|]+/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, SNIPPET_CHARS);

interface OutlineEntry {
  turn: SessionTurn;
  question?: string;
  reply?: string;
  steps: number;
  failed: boolean;
}

export interface SessionOutlineProps {
  turns: SessionTurn[];
  /** 正文当前停在哪一轮（turns 下标），高亮并滚到可见 */
  activeTurn: number;
  onJumpQuestion: (turn: SessionTurn) => void;
  onJumpReply: (turn: SessionTurn) => void;
}

/**
 * 右侧对话目录：每轮拆成「你」和 Agent 两条，和正文一样人靠右、Agent 靠左，
 * 一眼分清谁说了什么；点哪条跳到正文对应位置，滚动正文时跟着高亮当前轮。
 */
export const SessionOutline = memo(function SessionOutline({
  turns,
  activeTurn,
  onJumpQuestion,
  onJumpReply,
}: SessionOutlineProps) {
  const { t } = useTranslation();
  const rt = useReaderT();
  const listRef = useRef<HTMLDivElement>(null);

  const entries = useMemo<OutlineEntry[]>(
    () =>
      turns
        // 只有事件（模型切换、压缩等）、没有提问也没有 Agent 输出的轮不进目录
        .filter(
          (turn) =>
            turn.question ||
            turn.final ||
            turn.steps.length > 0 ||
            turn.aborted,
        )
        .map((turn) => ({
          turn,
          question: turn.question ? toSnippet(turn.question.text) : undefined,
          reply: turn.final ? toSnippet(turn.final.text) : undefined,
          steps: turn.steps.length,
          failed: turn.aborted || turn.steps.some(isFailureStep),
        })),
    [turns],
  );

  const virtualizer = useVirtualizer({
    count: entries.length,
    getScrollElement: () => listRef.current,
    estimateSize: () => 112,
    getItemKey: (index) => entries[index]?.turn.key ?? index,
    overscan: 6,
  });
  const activeIndex = useMemo(
    () => entries.findIndex((entry) => entry.turn.index === activeTurn),
    [entries, activeTurn],
  );

  // 目录项可能尚未挂载，按索引滚动而不是查询 DOM。
  useEffect(() => {
    if (activeIndex >= 0) {
      virtualizer.scrollToIndex(activeIndex, { align: "auto" });
    }
  }, [activeIndex, virtualizer]);

  return (
    <nav
      aria-label={rt("outlineTitle")}
      className="flex min-h-0 w-[260px] shrink-0 flex-col border-s border-border max-lg:hidden"
    >
      <div className="flex h-9 shrink-0 items-center px-4 text-caption font-semibold text-fg-2">
        {rt("outlineTitle")}
        <span className="ms-1.5 font-normal tabular-nums text-fg-3">
          {entries.length}
        </span>
      </div>
      <div
        ref={listRef}
        className="min-h-0 flex-1 overflow-y-auto scroll-stable overscroll-contain px-2 pb-4"
      >
        <ol
          className="relative m-0 list-none p-0"
          style={{ height: virtualizer.getTotalSize() }}
        >
          {virtualizer.getVirtualItems().map((item) => {
            const entry = entries[item.index];
            if (!entry) return null;
            const active = entry.turn.index === activeTurn;
            return (
              <li
                key={item.key}
                data-index={item.index}
                data-outline-turn={entry.turn.index}
                ref={virtualizer.measureElement}
                style={{
                  position: "absolute", top: 0, left: 0, width: "100%",
                  transform: `translateY(${item.start}px)`,
                }}
                className={cn(
                  "flex flex-col gap-1 rounded-panel px-1.5 pt-1.5 pb-2.5 transition-colors",
                  active && "bg-subtle",
                )}
              >
                {active && (
                  <span
                    aria-hidden
                    className="absolute inset-y-2 start-0 w-[2px] rounded-full bg-[var(--reader-accent)]"
                  />
                )}

                {entry.question !== undefined && (
                  <button
                    type="button"
                    onClick={() => onJumpQuestion(entry.turn)}
                    aria-current={active ? "true" : undefined}
                    className="group/outline flex min-w-0 flex-col items-end gap-0.5 self-end rounded-control text-end focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  >
                    <span className="text-[11px] tabular-nums text-fg-3">
                      {t("sessionManager.you", { defaultValue: "你" })}
                      {entry.turn.question?.ts
                        ? ` · ${formatMessageTime(entry.turn.question.ts)}`
                        : ""}
                    </span>
                    {/* 人的输入：靠右的小气泡，和正文提问同色 */}
                    <span
                      className="line-clamp-2 max-w-[200px] break-words rounded-[10px] rounded-se-[3px] px-2 py-1 text-start text-caption text-fg-1 transition-[filter] group-hover/outline:brightness-110"
                      style={{
                        background:
                          "color-mix(in srgb, var(--reader-accent) 14%, var(--bg-subtle))",
                      }}
                    >
                      {entry.question || rt("outlineEmptyQuestion")}
                    </span>
                  </button>
                )}

                <button
                  type="button"
                  onClick={() => onJumpReply(entry.turn)}
                  className="group/outline flex min-w-0 items-start gap-1.5 rounded-control text-start focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                >
                  {/* AI 的输出：靠左，带 Agent 头像，正文不加底色 */}
                  <span className="mt-0.5 origin-top-left scale-[0.72]">
                    <SessionAgentAvatar />
                  </span>
                  <span className="-ms-1.5 flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="flex items-center gap-1 text-[11px] tabular-nums text-fg-3">
                      {entry.steps > 0 &&
                        rt("outlineSteps", { count: entry.steps })}
                      {entry.failed && (
                        <span
                          aria-label={rt("outlineFailed")}
                          className="h-1.5 w-1.5 rounded-full bg-danger"
                        />
                      )}
                    </span>
                    <span
                      className={cn(
                        "line-clamp-2 break-words text-caption transition-colors group-hover/outline:text-fg-1",
                        entry.reply ? "text-fg-2" : "italic text-fg-3",
                      )}
                    >
                      {entry.reply ||
                        (entry.turn.aborted
                          ? rt("outlineAborted")
                          : rt("outlineNoReply"))}
                    </span>
                  </span>
                </button>
              </li>
            );
          })}
        </ol>
      </div>
    </nav>
  );
});
