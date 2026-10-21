import deepseekArtwork from "../../assets/skins/deepseek.png";
import claudeArtwork from "../../assets/skins/claude.png";
import gptArtwork from "../../assets/skins/gpt.png";

export const INTERFACE_SKINS = [
  {
    id: "deepseek",
    name: { zh: "DeepSeek · 蓝海女仆", en: "DeepSeek · Ocean maid" },
    artwork: deepseekArtwork,
    tint: "#5c85ee24",
    faceAnchor: { x: 49, y: 20 },
  },
  {
    id: "claude",
    name: { zh: "Claude · 暖橙书卷", en: "Claude · Amber scholar" },
    artwork: claudeArtwork,
    tint: "#da6d4a24",
    faceAnchor: { x: 50, y: 11 },
  },
  {
    id: "gpt",
    name: { zh: "GPT · 银白龙姬", en: "GPT · Silver dragon" },
    artwork: gptArtwork,
    tint: "#a081de24",
    faceAnchor: { x: 49, y: 14 },
  },
] as const;

export type InterfaceSkinId = "none" | (typeof INTERFACE_SKINS)[number]["id"];

export function getInterfaceSkin(value: unknown) {
  return INTERFACE_SKINS.find((skin) => skin.id === value);
}

export function normalizeInterfaceSkin(value: unknown): InterfaceSkinId {
  return getInterfaceSkin(value)?.id ?? "none";
}
