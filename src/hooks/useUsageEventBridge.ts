import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
export function useUsageEventBridge() {
  const client = useQueryClient();
  useEffect(() => {
    const refresh = () => {
      void client.invalidateQueries({ queryKey: ["usage"] });
    };
    window.addEventListener("harness-scope:changed", refresh);
    return () => window.removeEventListener("harness-scope:changed", refresh);
  }, [client]);
}
