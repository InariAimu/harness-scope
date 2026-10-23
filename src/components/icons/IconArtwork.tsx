import { useState, type ReactNode } from "react";
import { ICON_PRESETS, type IconLibrary, type IconRef } from "@/lib/entityIcons";

/** Only bundled SVGs are inline; imported SVG/PNG assets use the image sandbox. */
export function IconArtwork({ icon, fallback, placeholder, library, size = 20, fill = false }: {
  icon?: IconRef; fallback?: IconRef; placeholder?: ReactNode; library?: IconLibrary; size?: number; fill?: boolean;
}) {
  const [failed, setFailed] = useState<string>();
  const asset = icon?.startsWith("asset:") ? library?.assets[icon.slice(6)] : undefined;
  const available = (ref?: IconRef) => ICON_PRESETS.find(p => `builtin:${p.id}` === ref && (!p.src || p.src !== failed));
  const builtin = available(icon) ?? available(fallback);
  const src = asset && failed !== asset.dataUrl ? asset.dataUrl : builtin?.src;
  if (src) {
    return <img src={src} alt="" aria-hidden="true" draggable={false}
      width={fill ? undefined : size} height={fill ? undefined : size}
      className={fill ? "absolute inset-0 h-full w-full rounded-[inherit] object-cover" : "shrink-0 object-contain"}
      style={fill ? undefined : { width: size, height: size }}
      onError={() => setFailed(src)} />;
  }
  if (builtin?.svg) return <span aria-hidden="true" className="inline-flex shrink-0 items-center justify-center [&>svg]:h-full [&>svg]:w-full"
    style={{ width: size, height: size, fontSize: size, lineHeight: 1 }} dangerouslySetInnerHTML={{ __html: builtin.svg }} />;
  return <>{placeholder}</>;
}
