// Badge layout from CC Switch e8880e6; brand provenance in third_party/UPSTREAM.md.
import { Monitor, Terminal, Code2 } from "lucide-react";
import type { AppId } from "@/lib/api";
import { cn } from "@/lib/utils";
import { configuredIcon, defaultHarnessIcon, useIconLibrary } from "@/lib/entityIcons";
import { IconArtwork } from "@/components/icons/IconArtwork";
export const APP_DISPLAY_NAME: Record<AppId, string> = {
  claude: "Claude Code",
  "claude-desktop": "Claude Desktop",
  codex: "Codex",
  gemini: "Gemini CLI",
  grokbuild: "Grok Build",
  opencode: "OpenCode",
  dsh: "DeepSeek Harness",
  cherry: "Cherry Studio",
  lmstudio: "LM Studio",
  "copliot-deprecated": "copliot-deprecated",
  copliot: "Github Copliot",
  openclaw: "OpenClaw",
  hermes: "Hermes",
  pi: "Pi",
  mcode: "MiniMax Code",
};
const BADGE: Record<string, typeof Terminal | undefined> = {
  claude: Terminal,
  "claude-desktop": Monitor,
};
export function AppGlyph({
  app,
  size = 16,
  fill = false,
  badgeClassName,
  className,
}: {
  app: AppId;
  size?: number;
  fill?: boolean;
  badgeClassName?: string;
  className?: string;
}) {
  const { data } = useIconLibrary();
  const Badge = BADGE[app];
  return (
    <span
      aria-hidden="true"
      className={cn(
        "relative inline-flex shrink-0 items-center justify-center",
        fill && "h-full w-full rounded-[inherit]",
        className,
      )}
      style={fill ? undefined : { width: size + 2, height: size + 2 }}
    >
      <span className={cn("inline-flex", fill && "absolute inset-0 items-center justify-center overflow-hidden rounded-[inherit]", (app === "codex" || app === "copliot") && "text-fg-1", app === "copliot-deprecated" && "text-muted-foreground")}>
        <IconArtwork icon={configuredIcon(data, { kind: "harness", id: app })} fallback={defaultHarnessIcon(app)}
          library={data} size={size} fill={fill} placeholder={<Code2 size={size} strokeWidth={1.8} />} />
      </span>
      {Badge && (
        <span
          className={cn(
            "absolute -bottom-1 -right-1 flex h-[11px] w-[11px] items-center justify-center rounded-[3px] bg-sidebar text-fg-2",
            fill && "bottom-0 right-0",
            badgeClassName,
          )}
        >
          <Badge className="h-[9px] w-[9px]" strokeWidth={3} />
        </span>
      )}
    </span>
  );
}
