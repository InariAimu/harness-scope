// Adapted from CC Switch e8880e6f57e25bce1bf3125cc6e33d9fb2b0d806 (MIT).
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Maximize2, Minimize2, Minus, X } from "lucide-react";
import { toast } from "@/lib/toast";
import { cn } from "@/lib/utils";
import { HoverTip } from "@/components/ui/hover-tip";

const BUTTON =
  "flex h-7 w-7 items-center justify-center rounded-control text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1";

export function WindowControls() {
  const { t } = useTranslation();
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    const win = getCurrentWindow();
    const update = async () => {
      const value = await win.isMaximized();
      if (active) setMaximized(value);
    };
    void update().catch(console.error);
    void win
      .onResized(() => void update().catch(console.error))
      .then((stop) => {
        if (active) unlisten = stop;
        else stop();
      })
      .catch(console.error);
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);
  const run = async (action: () => Promise<void>) => {
    try {
      await action();
    } catch (error) {
      toast.error(
        t("notifications.windowControlFailed", {
          defaultValue: "窗口控制失败：{{error}}",
          error: String(error),
        }),
      );
    }
  };
  const maximizeLabel = maximized
    ? t("header.windowRestore")
    : t("header.windowMaximize");
  return (
    <>
      <HoverTip content={t("header.windowMinimize")}>
        <button
          type="button"
          className={BUTTON}
          aria-label={t("header.windowMinimize")}
          onClick={() => void run(() => getCurrentWindow().minimize())}
        >
          <Minus className="h-4 w-4" />
        </button>
      </HoverTip>
      <HoverTip content={maximizeLabel}>
        <button
          type="button"
          className={BUTTON}
          aria-label={maximizeLabel}
          onClick={() =>
            void run(async () => {
              const win = getCurrentWindow();
              await win.toggleMaximize();
              setMaximized(await win.isMaximized());
            })
          }
        >
          {maximized ? (
            <Minimize2 className="h-4 w-4" />
          ) : (
            <Maximize2 className="h-4 w-4" />
          )}
        </button>
      </HoverTip>
      <HoverTip content={t("header.windowClose")}>
        <button
          type="button"
          className={cn(BUTTON, "hover:bg-danger-soft hover:text-danger-text")}
          aria-label={t("header.windowClose")}
          onClick={() => void run(() => getCurrentWindow().close())}
        >
          <X className="h-4 w-4" />
        </button>
      </HoverTip>
    </>
  );
}
