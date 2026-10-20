import { command } from "@/lib/client";
import type {
  GoCatalog,
  GoCatalogInfo,
  GoPlan,
  GoSimulation,
  GoTokens,
  GoUsageEstimate,
} from "@/types/goCatalog";
export const goCatalogApi = {
  info: () => command<GoCatalogInfo>("go_catalog"),
  save: (catalog: GoCatalog, info: GoCatalogInfo) =>
    command<GoCatalogInfo>("save_go_catalog", {
      catalog,
      expectedRevision: info.revision,
      expectedFileRevision: info.file.revision,
    }),
  usage: (id: string, plan: GoPlan) =>
    command<GoUsageEstimate>("go_usage_estimate", { id, plan }),
  simulate: (args: {
    model: string;
    plan: GoPlan;
    requests: number;
    tokens: GoTokens;
    period: string;
  }) => command<GoSimulation>("go_simulate", args),
};
