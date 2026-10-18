import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { UsageDisplaySettings } from "@/types";
import { command } from "@/lib/client";
import { toast } from "@/lib/toast";

const key = ["usage-display"] as const;
let pendingSave: Promise<unknown> = Promise.resolve();

export function useUsageDisplay() {
  const client = useQueryClient();
  const query = useQuery({
    queryKey: key,
    queryFn: () => command<UsageDisplaySettings>("usage_display"),
    staleTime: Infinity,
  });
  const save = async (patch: Partial<UsageDisplaySettings>) => {
    // Serialize patches; changing one control cannot overwrite another control's save.
    const request = pendingSave.then(() =>
      command<UsageDisplaySettings>("save_usage_display", { patch }),
    );
    pendingSave = request.catch(() => undefined);
    try {
      const display = await request;
      client.setQueryData(key, display);
      return true;
    } catch (error) {
      toast.error(String(error));
      return false;
    }
  };
  return { ...query, save };
}
