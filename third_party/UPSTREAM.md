# CC Switch source provenance

Upstream: https://github.com/farion1231/cc-switch
Pinned commit: e8880e6f57e25bce1bf3125cc6e33d9fb2b0d806
License: MIT, Copyright (c) 2025 Jason Young; see CC-Switch-LICENSE.

Copied frontend files (subsequently adapted for standalone data APIs, paths, pricing provenance and unknown measurements):

- `postcss.config.cjs`
- `src/components/ConfirmDialog.tsx`
- `src/components/common/FullScreenPanel.tsx`
- `src/components/sessions/SessionItem.tsx`
- `src/components/sessions/reader/PathChip.tsx`
- `src/components/sessions/reader/SessionAgentAvatar.tsx`
- `src/components/sessions/reader/SessionCodeBlock.tsx`
- `src/components/sessions/reader/SessionDiff.tsx`
- `src/components/sessions/reader/SessionEventRow.tsx`
- `src/components/sessions/reader/SessionFinalReply.tsx`
- `src/components/sessions/reader/SessionImage.tsx`
- `src/components/sessions/reader/SessionImageLightbox.tsx`
- `src/components/sessions/reader/SessionMarkdown.tsx`
- `src/components/sessions/reader/SessionNoteRow.tsx`
- `src/components/sessions/reader/SessionOutline.tsx`
- `src/components/sessions/reader/SessionQuestion.tsx`
- `src/components/sessions/reader/SessionReader.tsx`
- `src/components/sessions/reader/SessionReaderHeader.tsx`
- `src/components/sessions/reader/SessionStep.tsx`
- `src/components/sessions/reader/SessionStepDetails.tsx`
- `src/components/sessions/reader/SessionThinkingRow.tsx`
- `src/components/sessions/reader/SessionTimeline.tsx`
- `src/components/sessions/reader/agentStyles.ts`
- `src/components/sessions/reader/context.ts`
- `src/components/sessions/reader/exportMarkdown.ts`
- `src/components/sessions/reader/i18n.ts`
- `src/components/sessions/reader/rowStyles.tsx`
- `src/components/sessions/reader/toolSummary.ts`
- `src/components/sessions/reader/turns.ts`
- `src/components/sessions/utils.ts`
- `src/components/shell/AppPageHeader.tsx`
- `src/components/ui/accordion.tsx`
- `src/components/ui/alert.tsx`
- `src/components/ui/badge.tsx`
- `src/components/ui/button.tsx`
- `src/components/ui/card.tsx`
- `src/components/ui/checkbox.tsx`
- `src/components/ui/collapsible.tsx`
- `src/components/ui/command.tsx`
- `src/components/ui/dialog.tsx`
- `src/components/ui/dropdown-menu.tsx`
- `src/components/ui/form.tsx`
- `src/components/ui/help-tip.tsx`
- `src/components/ui/hover-tip.tsx`
- `src/components/ui/ime-safe-input.tsx`
- `src/components/ui/input.tsx`
- `src/components/ui/label.tsx`
- `src/components/ui/notice.tsx`
- `src/components/ui/page-tabs.tsx`
- `src/components/ui/popover.tsx`
- `src/components/ui/scroll-area.tsx`
- `src/components/ui/search-field.tsx`
- `src/components/ui/segmented-control.tsx`
- `src/components/ui/select.tsx`
- `src/components/ui/sheet.tsx`
- `src/components/ui/sliding-indicator.tsx`
- `src/components/ui/sonner.tsx`
- `src/components/ui/switch.tsx`
- `src/components/ui/table.tsx`
- `src/components/ui/tabs.tsx`
- `src/components/ui/textarea.tsx`
- `src/components/ui/toggle-row.tsx`
- `src/components/ui/tooltip.tsx`
- `src/components/usage/ModelStatsTable.tsx`
- `src/components/usage/ModelsDevAutoSyncPanel.tsx`
- `src/components/usage/ModelsDevPickerDialog.tsx`
- `src/components/usage/PricingConfigPanel.tsx`
- `src/components/usage/PricingEditModal.tsx`
- `src/components/usage/ProviderStatsTable.tsx`
- `src/components/usage/RequestDetailPanel.tsx`
- `src/components/usage/RequestLogTable.tsx`
- `src/components/usage/TablePagination.tsx`
- `src/components/usage/UsageDashboard.tsx`
- `src/components/usage/UsageDataSourcesSheet.tsx`
- `src/components/usage/UsageDateRangePicker.tsx`
- `src/components/usage/UsageHeatmap.tsx`
- `src/components/usage/UsageHero.tsx`
- `src/components/usage/UsagePage.tsx`
- `src/components/usage/UsageTooltipCard.tsx`
- `src/components/usage/UsageTrendChart.tsx`
- `src/components/usage/format.ts`
- `src/components/usage/providerLabel.ts`
- `src/components/usage/statsColumns.tsx`
- `src/components/usage/usageTable.ts`
- `src/index.css`
- `src/lib/clipboard.ts`
- `src/lib/modelsDev.ts`
- `src/lib/modelsDevAutoSync.ts`
- `src/lib/modelsDevPricing.ts`
- `src/lib/platform.ts`
- `src/lib/toast.ts`
- `src/lib/usageRange.ts`
- `src/lib/utils.ts`
- `src/types/usage.ts`
- `src/utils/domUtils.ts`
- `src/utils/errorUtils.ts`
- `tailwind.config.cjs`
- `src/i18n/locales/{zh,en}.json`
- Session types excerpt from `src/types.ts`.
- Built-in pricing tuples extracted from `src-tauri/src/database/schema.rs` into `crates/core/resources/builtin-pricing.json`.

No routing, provider configuration or authentication code is used.

Additional copied support: `src/lib/inputModality.ts`, `src/lib/unsavedChanges.ts`, `src/lib/query/usage.ts`, `src/hooks/useUsageAutoRefresh.ts`. Model-name candidate and prefix-match helpers from `src-tauri/src/services/usage_stats.rs` are adapted in `crates/core/src/pricing_match.rs` (the context marker is a local literal). The pricing panel and source sheet are replaced with standalone versions. Copied unused `ui/form.tsx`, `ui/sonner.tsx`, and `usage/UsagePage.tsx` were removed.

2026-10-17 shell alignment: `src/components/shell/WindowControls.tsx` adapts the upstream component with safe resize-listener cleanup. `AppGlyph.tsx` follows upstream brand mapping and Claude terminal/desktop badges. The Claude, OpenAI (Codex), and OpenCode SVG strings from `src/icons/extracted/index.ts` at the pinned commit are vendored in `src/assets/harness/{claude,openai,opencode}.svg`; artwork is unchanged. Other unregistered brands retain a generic fallback. The existing CC Switch MIT license covers these copied assets and components.

2026-10-17 timing: `crates/core/src/harness/codex_timing.rs` adapts the pinned `src-tauri/src/services/session_usage_codex.rs` request timer and `session_usage.rs` plausibility checks. OpenCode streamed/completed precedence follows `session_usage_opencode.rs`. Weighted speed eligibility and first-token median follow `usage_stats.rs` and the vendored frontend formulas. OpenCode content-based first-token estimates and local timing storage are Harness Scope additions.

2026-10-18 usage settings alignment: this feature uses CC Switch **v4.0.14**, commit **c1caee80d7f047e0d671292a0cc0502cc07ea1e5** ([tag](https://github.com/farion1231/cc-switch/tree/v4.0.14)). `UsageDisplaySection.tsx`, `requestLogColumns.ts` and `RequestLogTable.tsx` are copied/adapted from that release. Currency/sidebar helpers in `usage/format.ts`, `hooks/useMoneyFormat.ts`, usage settings section layout, sidebar Today presentation and the relevant `usage.display`, `usage.logColumns`, `usage.tabs.settings`, `nav.today*` translations follow the same tag. Persistence uses Harness Scope device-local metadata and field patches instead of CC Switch settings APIs; unknown statuses and native cache-write counters remain supported. Other vendored components and the pricing baseline remain on the original pinned commit. All are covered by the CC Switch MIT license.

2026-10-19 trend chart styles: the bar/area selection and display-settings grid in `UsageDisplaySection.tsx`, rendering in `UsageTrendChart.tsx`, `TREND_CHART_TYPES` / `resolveTrendChartType` in `usage/format.ts`, `UsageDisplaySettings.trendChartType`, and the Chinese/English `usage.display.trendChart*` translations follow CC Switch **v4.0.15**, commit **79e754ab1b6ed50da57dae1d0faecab4b3b52f1c** ([tag](https://github.com/farion1231/cc-switch/tree/v4.0.15), [upstream PR #8406](https://github.com/farion1231/cc-switch/pull/8406)). The local query hook, validated metadata patches, pending-save handling and failed-save rollback are retained. The original pricing baseline and unrelated vendored files remain unchanged. Covered by the existing CC Switch MIT license.

2026-10-19 session reading performance: bounded head/tail metadata reads, compressed Codex header reads, metadata fingerprints, transcript LRU limits (8 / 96 MiB), message batch limits (150 / 256 KiB), and lazy tool/thinking previews follow the architecture in v4.0.15 `session_manager/providers/{utils,codex,opencode,blocks}.rs`, `session_manager/cache.rs`, and the session query hooks. Harness Scope independently implements adapter reader projections, snapshot-bound block references and transport-neutral paged commands shared by Tauri and the local HTTP API. Complete usage parsing remains separate; no additional CC Switch runtime package is bundled.

2026-10-19 usage startup: persistent file size/modified-time checkpoints and prefix/tail rewrite checks follow the approach in v4.0.15 `services/session_usage{,_codex}.rs`. The usage-only streaming JSON projection, WAL-aware checkpoints, and direct-parent timeline invalidation are implemented locally. No upstream scanner code is copied.

2026-10-19 trend time axis: dense zero-filled hourly/day buckets in `crates/core/src/query.rs` follow v4.0.15 `services/usage_stats.rs::get_daily_trends` at `79e754ab1b6ed50da57dae1d0faecab4b3b52f1c`: hourly intervals are anchored to the requested start, an inclusive end is assigned to the final interval, and longer ranges iterate local calendar dates. RFC3339 bucket keys include offsets. Harness Scope adds configured-IANA-zone handling across range selection, chart labels and calendars, keeps an exact-midnight range at midnight, and retains its existing complete-day rule for migrated rollups. Covered by the existing CC Switch MIT license for the adapted algorithm.

## DeepSeek brand asset

`src/assets/harness/deepseek.svg` uses the official blue whale from [deepseek-ai/awesome-deepseek-integration, ICON.svg](https://github.com/deepseek-ai/awesome-deepseek-integration/blob/94a834802fd1489db4ced41f3a23c669cb8644b1/docs/_logo%20svg/ICON.svg), commit `94a834802fd1489db4ced41f3a23c669cb8644b1`. Original SHA-256: `b15083e3cbb398bd35619b758c43adc0a632efccc7e2d69f2481a860862910fe`. Official geometry, transform, viewBox and `#4d6bfe` color are preserved; dimensions are set to `1em`, editor IDs removed and CSS fill inlined to avoid collisions between injected SVGs. This identifies the DeepSeek Harness data source. DeepSeek artwork follows its [official logo guidelines](https://github.com/deepseek-ai/awesome-deepseek-integration/blob/94a834802fd1489db4ced41f3a23c669cb8644b1/docs/_logo%20svg/guidelines.md); it is not covered by the CC Switch provenance above.
