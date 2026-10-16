import { invoke, isTauri } from "@tauri-apps/api/core";
export async function command<T>(name: string, args: unknown = {}): Promise<T> {
  if (isTauri()) return invoke<T>("command", { name, args });
  if (name === "open_external") {
    const url = new URL((args as { url: string }).url);
    if (!["http:", "https:"].includes(url.protocol))
      throw new Error("Unsupported link");
    window.open(url.href, "_blank", "noopener,noreferrer");
    return true as T;
  }
  const response = await fetch("/api/command", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name, args }),
  });
  const value = await response
    .json()
    .catch(() => ({
      error: "Local API is unavailable. Start the local service on port 8788.",
    }));
  if (!response.ok) throw new Error(value.error || `HTTP ${response.status}`);
  return value as T;
}
let scope = "local";
export const getScope = () => scope;
export const setScope = (value: string) => {
  scope = value;
};

export async function saveText(
  name: string,
  content: string,
): Promise<string | null> {
  if (isTauri()) {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const path = await save({
      defaultPath: name,
      filters: [{ name: "Markdown", extensions: ["md"] }],
    });
    if (!path) return null;
    await command("export_text", { path, content });
    return path;
  }
  const url = URL.createObjectURL(
    new Blob([content], { type: "text/markdown;charset=utf-8" }),
  );
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
  return name;
}
