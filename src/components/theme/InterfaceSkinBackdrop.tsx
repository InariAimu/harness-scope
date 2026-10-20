import deepseekArtwork from "../../../assets/skins/deepseek.png";
import { useInterfaceSkin } from "./AccentThemeProvider";

export function InterfaceSkinBackdrop() {
  if (useInterfaceSkin() !== "deepseek") return null;
  return (
    <div className="interface-skin-backdrop" aria-hidden="true">
      <img
        className="interface-skin-character"
        src={deepseekArtwork}
        alt=""
        draggable={false}
      />
      <div className="interface-skin-veil" />
    </div>
  );
}
