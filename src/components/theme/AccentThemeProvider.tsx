import {
  createContext,
  useContext,
  useLayoutEffect,
  useState,
  type ReactNode,
  type Dispatch,
  type SetStateAction,
} from "react";
import { accentColorVariables } from "@/lib/accentColor";
import { skinAppearanceVariables } from "@/lib/skinAppearance";
import { normalizeInterfaceSkin, type InterfaceSkinId } from "@/lib/interfaceSkins";

interface ThemePreview {
  accentColor: string;
  theme: string;
  interfaceSkin: InterfaceSkinId;
  skinTransparency: number;
  skinMaskStrength: number;
}

const SkinContext = createContext<InterfaceSkinId>("none");
export const useInterfaceSkin = () => useContext(SkinContext);

const PreviewContext = createContext<
  Dispatch<SetStateAction<ThemePreview | null>>
>(() => {});

export const useAccentThemePreview = () => useContext(PreviewContext);

export function AccentThemeProvider({
  color,
  theme = "system",
  skin = "none",
  skinTransparency,
  skinMaskStrength,
  children,
}: {
  color?: string;
  theme?: string;
  skin?: string;
  skinTransparency?: number;
  skinMaskStrength?: number;
  children: ReactNode;
}) {
  const [preview, setPreview] = useState<ThemePreview | null>(null);
  const effectiveColor = preview?.accentColor ?? color;
  const effectiveTheme = preview?.theme ?? theme;
  const effectiveSkin = normalizeInterfaceSkin(preview?.interfaceSkin ?? skin);
  const effectiveTransparency = preview?.skinTransparency ?? skinTransparency;
  const effectiveMaskStrength = preview?.skinMaskStrength ?? skinMaskStrength;

  useLayoutEffect(() => {
    const root = document.documentElement;
    const variables = skinAppearanceVariables(effectiveTransparency, effectiveMaskStrength);
    for (const [name, value] of Object.entries(variables)) {
      root.style.setProperty(name, value);
    }
    return () => {
      for (const name of Object.keys(variables)) root.style.removeProperty(name);
    };
  }, [effectiveTransparency, effectiveMaskStrength]);

  useLayoutEffect(() => {
    document.documentElement.dataset.interfaceSkin = effectiveSkin;
    return () => { delete document.documentElement.dataset.interfaceSkin; };
  }, [effectiveSkin]);

  useLayoutEffect(() => {
    const root = document.documentElement;
    for (const [name, value] of Object.entries(
      accentColorVariables(effectiveColor),
    )) {
      root.style.setProperty(name, value);
    }
  }, [effectiveColor]);

  useLayoutEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const update = () =>
      document.documentElement.classList.toggle(
        "dark",
        effectiveTheme === "dark" ||
          (effectiveTheme === "system" && media.matches),
      );
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, [effectiveTheme]);

  return (
    <PreviewContext.Provider value={setPreview}>
      <SkinContext.Provider value={effectiveSkin}>{children}</SkinContext.Provider>
    </PreviewContext.Provider>
  );
}
