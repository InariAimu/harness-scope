// Badge layout from CC Switch e8880e6; brand provenance in third_party/UPSTREAM.md.
import { Monitor, Terminal, Code2 } from "lucide-react";
import type { AppId } from "@/lib/api";
import { cn } from "@/lib/utils";
import claude from "@/assets/harness/claude.svg?raw";
import openai from "@/assets/harness/openai.svg?raw";
import opencode from "@/assets/harness/opencode.svg?raw";
import deepseek from "@/assets/harness/deepseek.svg?raw";
export const APP_DISPLAY_NAME: Record<AppId, string> = {
  claude: "Claude Code",
  "claude-desktop": "Claude Desktop",
  codex: "Codex",
  gemini: "Gemini CLI",
  grokbuild: "Grok Build",
  opencode: "OpenCode",
  dsh: "DeepSeek Harness",
  openclaw: "OpenClaw",
  hermes: "Hermes",
  pi: "Pi",
  mcode: "MiniMax Code",
};
const BRAND_SVG: Record<string, string> = {
  claude,
  "claude-desktop": claude,
  codex: openai,
  opencode,
  dsh: deepseek,
};
const BADGE: Record<string, typeof Terminal | undefined> = {
  claude: Terminal,
  "claude-desktop": Monitor,
};
export function AppGlyph({
  app,
  size = 16,
  badgeClassName,
  className,
}: {
  app: AppId;
  size?: number;
  badgeClassName?: string;
  className?: string;
}) {
  const svg = BRAND_SVG[app];
  const Badge = BADGE[app];
  return (
    <span
      aria-hidden="true"
      className={cn(
        "relative inline-flex shrink-0 items-center justify-center",
        className,
      )}
      style={{ width: size + 2, height: size + 2 }}
    >
      {svg ? (
        <span
          className="inline-flex items-center justify-center"
          style={{ width: size, height: size, fontSize: size, lineHeight: 1 }}
          dangerouslySetInnerHTML={{ __html: svg }}
        />
      ) : (
        <Code2 size={size} strokeWidth={1.8} />
      )}
      {Badge && (
        <span
          className={cn(
            "absolute -bottom-1 -right-1 flex h-[11px] w-[11px] items-center justify-center rounded-[3px] bg-sidebar text-fg-2",
            badgeClassName,
          )}
        >
          <Badge className="h-[9px] w-[9px]" strokeWidth={3} />
        </span>
      )}
    </span>
  );
}
