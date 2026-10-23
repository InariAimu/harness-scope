import type { QueryClient } from "@tanstack/react-query";
import type { Snapshot } from "@/App";
import { command } from "./client";

export interface ApplicationSettings {
  collectionEnabled: boolean;
  visible: boolean;
}
export type ApplicationSettingsMap = Record<string, ApplicationSettings>;

export function applicationSettings(
  settings: ApplicationSettingsMap | undefined,
  harness: string,
): ApplicationSettings {
  return settings?.[harness] ?? { collectionEnabled: true, visible: true };
}

export async function saveApplicationSettings(
  client: QueryClient,
  harness: string,
  patch: Partial<ApplicationSettings>,
) {
  const updated = await command<ApplicationSettings>("save_application_settings", {
    harness,
    patch,
  });
  // An older snapshot response must not restore the previous switch position.
  await client.cancelQueries({ queryKey: ["snapshot"] });
  client.setQueryData<Snapshot>(["snapshot"], (snapshot) =>
    snapshot && {
      ...snapshot,
      applicationSettings: { ...snapshot.applicationSettings, [harness]: updated },
    },
  );
  // Collection controls whether a session can be safely deleted. Visibility
  // only changes filter choices, so it must not clear usage or session data.
  if (patch.collectionEnabled !== undefined) {
    await client.resetQueries({ queryKey: ["sessions"] });
  }
  await client.invalidateQueries({ queryKey: ["snapshot"] });
  return updated;
}
