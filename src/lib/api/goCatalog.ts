import { command } from "@/lib/client";
import type {
  GoCatalog,
  GoCatalogInfo,
  GoPlan,
  GoSimulation,
  GoTokens,
  GoUsageEstimate,
  GoForecast,
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
  forecast: (args: {
    id: string;
    plan: GoPlan;
    model?: string;
    lookbackMinutes: number;
    pace: string;
  }) => command<GoForecast>("go_forecast", args),
  simulate: (args: {
    model: string;
    plan: GoPlan;
    requests: number;
    tokens: GoTokens;
    period: string;
  }) => command<GoSimulation>("go_simulate", args),
};
