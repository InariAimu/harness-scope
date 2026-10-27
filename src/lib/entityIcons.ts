import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { command } from "./client";
import claude from "@/assets/harness/claude.svg?raw";
import openai from "@/assets/harness/openai.svg?raw";
import deepseek from "@/assets/harness/deepseek.svg?raw";
import opencode from "@/assets/harness/opencode.svg?raw";
import copilot from "@/assets/harness/copilot.svg?raw";
import presetCatalog from "../../crates/core/resources/icon-presets.json";

const brandArtwork: Record<string, string> = { claude, openai, deepseek, opencode, copilot };
const communityArtwork = import.meta.glob<string>("../../assets/icons/*.png", {
  eager: true, query: "?url", import: "default",
});
// The service validates against the same catalog, so every visible preset can be saved.
export const ICON_PRESETS = presetCatalog.map(preset => ({
  ...preset,
  svg: preset.group === "brand" ? brandArtwork[preset.id] : undefined,
  src: preset.file.endsWith(".png") ? communityArtwork[`../../assets/icons/${preset.file}`] : undefined,
}));
export type IconRef = `builtin:${string}` | `asset:${string}`;
export interface IconTarget { kind: "harness" | "account"; id: string }
export interface IconAsset { id: string; name: string; dataUrl: string; width: number; height: number }
export interface IconLibrary {
  revision: number;
  assets: Record<string, IconAsset>;
  harnesses: Record<string, IconRef>;
  accounts: Record<string, IconRef>;
}
export const ICON_QUERY_KEY = ["icon-library"] as const;
const HARNESS_DEFAULTS: Record<string, string> = {
  claude: "claude", "claude-desktop": "claude", codex: "openai", dsh: "deepseek",
  opencode: "opencode", cherry: "cherry", copliot: "copilot", "copliot-deprecated": "copilot",
};
const PROVIDER_DEFAULTS: Record<string, string> = {
  deepseek: "deepseek", "openai-codex": "openai", "opencode-go": "opencode",
};
export function defaultHarnessIcon(id: string): IconRef | undefined {
  return HARNESS_DEFAULTS[id] ? `builtin:${HARNESS_DEFAULTS[id]}` : undefined;
}
export function defaultProviderIcon(id: string): IconRef | undefined {
  return PROVIDER_DEFAULTS[id] ? `builtin:${PROVIDER_DEFAULTS[id]}` : undefined;
}
export function configuredIcon(library: IconLibrary | undefined, target: IconTarget) {
  return (target.kind === "harness" ? library?.harnesses : library?.accounts)?.[target.id];
}
export function iconUses(library: IconLibrary, id: string) {
  return [...Object.values(library.harnesses), ...Object.values(library.accounts)]
    .filter(value => value === `asset:${id}`).length;
}
export function useIconLibrary() {
  return useQuery({
    queryKey: ICON_QUERY_KEY,
    queryFn: () => command<IconLibrary>("icon_library"),
    staleTime: 30_000,
    refetchOnWindowFocus: true,
  });
}
type IconMutation =
  | { command: "set_entity_icon"; args: { target: IconTarget; icon: IconRef | null } }
  | { command: "import_entity_icon"; args: { target: IconTarget; name: string; dataUrl: string } }
  | { command: "delete_icon_asset"; args: { id: string } };
export function useIconMutation() {
  const client = useQueryClient();
  return useMutation({
    mutationKey: ICON_QUERY_KEY,
    // All pickers share this queue, so writes and responses retain their order.
    scope: { id: "icon-library" },
    mutationFn: async (request: IconMutation) => {
      await client.cancelQueries({ queryKey: ICON_QUERY_KEY });
      return command<IconLibrary>(request.command, request.args);
    },
    onSuccess: (updated) => {
      client.setQueryData<IconLibrary>(ICON_QUERY_KEY, current =>
        current && current.revision > updated.revision ? current : updated);
    },
  });
}
