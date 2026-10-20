import { useEffect } from "react";
import { providerAccountsApi } from "@/lib/api/providerAccounts";

// Only presence is reported by the UI. The backend owns all automatic remote polling.
export function useProviderActivity(id: string | undefined) {
  useEffect(() => {
    if (!id) return;
    const lease = crypto.randomUUID();
    let queue = Promise.resolve();
    const send = (active: boolean) => {
      queue = queue
        .then(async () => {
          await providerAccountsApi.activity(id, lease, active);
        })
        .catch(() => {});
    };
    const update = () =>
      send(document.visibilityState === "visible" && document.hasFocus());
    update();
    window.addEventListener("focus", update);
    window.addEventListener("blur", update);
    document.addEventListener("visibilitychange", update);
    const heartbeat = window.setInterval(update, 15_000);
    return () => {
      clearInterval(heartbeat);
      window.removeEventListener("focus", update);
      window.removeEventListener("blur", update);
      document.removeEventListener("visibilitychange", update);
      send(false);
    };
  }, [id]);
}
