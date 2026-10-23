import { Building2 } from "lucide-react";
import { configuredIcon, defaultProviderIcon, useIconLibrary } from "@/lib/entityIcons";
import { IconArtwork } from "./IconArtwork";

export function ProviderGlyph({ account, size = 16 }: { account: { id: string; provider: string }; size?: number }) {
  const { data } = useIconLibrary();
  return <span aria-hidden="true" className="inline-flex shrink-0 items-center justify-center" style={{ width: size, height: size }}>
    <IconArtwork icon={configuredIcon(data, { kind: "account", id: account.id })}
      fallback={defaultProviderIcon(account.provider)} library={data} size={size} placeholder={<Building2 size={size} />} />
  </span>;
}
