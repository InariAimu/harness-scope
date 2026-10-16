import { Terminal, Code2 } from "lucide-react";
import type { AppId } from "@/lib/api";
export const APP_DISPLAY_NAME: Record<AppId, string> = {
  claude: "Claude Code",
  "claude-desktop": "Claude Desktop",
  codex: "Codex",
  gemini: "Gemini CLI",
  grokbuild: "Grok Build",
  opencode: "OpenCode",
  openclaw: "OpenClaw",
  hermes: "Hermes",
  pi: "Pi",
  mcode: "MiniMax Code",
};
export function AppGlyph({
  app,
  size = 16,
  className,
}: {
  app: AppId;
  size?: number;
  badgeClassName?: string;
  className?: string;
}) {
  const Icon = app === "claude" ? Terminal : Code2;
  return (
    <Icon
      aria-hidden="true"
      size={size}
      className={className}
      strokeWidth={1.8}
    />
  );
}
