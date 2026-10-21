import type { CSSProperties } from "react";
import { getInterfaceSkin } from "@/lib/interfaceSkins";
import { useInterfaceSkin } from "./AccentThemeProvider";

export function InterfaceSkinBackdrop() {
  const skin = getInterfaceSkin(useInterfaceSkin());
  if (!skin) return null;
  return (
    <div
      className="interface-skin-backdrop"
      aria-hidden="true"
      style={{
        "--skin-tint": skin.tint,
        "--skin-face-x": `${-skin.faceAnchor.x}%`,
        "--skin-face-y": `${-skin.faceAnchor.y}%`,
      } as CSSProperties}
    >
      <img
        key={skin.id}
        className="interface-skin-character"
        src={skin.artwork}
        alt=""
        draggable={false}
      />
      <div className="interface-skin-veil" />
    </div>
  );
}
