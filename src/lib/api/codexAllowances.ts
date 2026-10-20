import { command } from "@/lib/client";
import type {
  CodexAllowanceCatalog,
  CodexAllowanceInfo,
} from "@/types/codexAllowances";
export const codexAllowancesApi = {
  info: () => command<CodexAllowanceInfo>("codex_allowance_catalog"),
  save: (catalog: CodexAllowanceCatalog, info: CodexAllowanceInfo) =>
    command<CodexAllowanceInfo>("save_codex_allowance_catalog", {
      catalog,
      expectedRevision: info.revision,
      expectedFileRevision: info.file.revision,
    }),
};
