import deepseekImage from "../../assets/deepseek.jpg";
import claudeImage from "../../assets/claude.jpg";
import geminiImage from "../../assets/gemini.jpg";
import gptImage from "../../assets/gpt.jpg";
import grokImage from "../../assets/grok.jpg";
import glmImage from "../../assets/glm.jpg";
import kimiImage from "../../assets/kimi.jpg";

interface CommunityAccentPreset {
  /** Stable identity for reusing the original artwork in future interface skins. */
  id: string;
  name: { zh: string; en: string };
  color: string;
  sourceImage: string;
  /** Background sample in original image pixels: [x, y, width, height]. */
  sampleRegion: readonly [number, number, number, number];
}

// Sampled from the supplied 1280×720 community artwork, not official brand colors.
// Each channel is the upper median of pixels sampled every 2 px in the region.
// Background regions avoid lettering and characters; the median rejects stray stars.
// Keep the original JPEGs intact so skins can reuse the full artwork later.
export const COMMUNITY_ACCENT_PRESETS = [
  {
    id: "deepseek",
    name: { zh: "DeepSeek 蓝", en: "DeepSeek Blue" },
    color: "#5c85ee",
    sourceImage: deepseekImage,
    sampleRegion: [220, 30, 240, 90],
  },
  {
    id: "claude",
    name: { zh: "Claude 橙", en: "Claude Orange" },
    color: "#da6d4a",
    sourceImage: claudeImage,
    sampleRegion: [1010, 180, 220, 170],
  },
  {
    id: "gemini",
    name: { zh: "Gemini 蓝紫", en: "Gemini Periwinkle" },
    color: "#8b86fd",
    sourceImage: geminiImage,
    sampleRegion: [220, 30, 240, 90],
  },
  {
    id: "gpt",
    name: { zh: "GPT 薰衣草紫", en: "GPT Lavender" },
    color: "#a081de",
    sourceImage: gptImage,
    sampleRegion: [850, 30, 240, 90],
  },
  {
    id: "grok",
    name: { zh: "Grok 金", en: "Grok Gold" },
    color: "#edb53e",
    sourceImage: grokImage,
    sampleRegion: [220, 30, 240, 90],
  },
  {
    id: "glm",
    name: { zh: "GLM 灰", en: "GLM Slate" },
    color: "#878d9f",
    sourceImage: glmImage,
    sampleRegion: [220, 30, 240, 90],
  },
  {
    id: "kimi",
    name: { zh: "Kimi 夜蓝", en: "Kimi Midnight" },
    color: "#182663",
    sourceImage: kimiImage,
    sampleRegion: [220, 30, 240, 90],
  },
] as const satisfies readonly CommunityAccentPreset[];
