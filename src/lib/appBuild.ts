export interface AppBuildInfo {
  version: string;
  displayVersion: string;
  channel: "release" | "dev";
  buildId: string;
  builtAt: string;
  gitCommit: string | null;
  dirty: boolean | null;
  profile?: string;
  target?: string;
}
declare const __APP_BUILD__: AppBuildInfo;
export const APP_BUILD: AppBuildInfo = __APP_BUILD__;
export function buildDescription(build: AppBuildInfo, zh: boolean) {
  return [
    `v${build.displayVersion}`,
    build.builtAt,
    build.gitCommit ?? (zh ? "Git 提交未知" : "Git commit unavailable"),
    build.dirty === true ? (zh ? "含未提交修改" : "Uncommitted changes")
      : build.dirty === false ? (zh ? "工作区干净" : "Clean checkout")
        : (zh ? "工作区状态未知" : "Checkout status unavailable"),
  ].join("\n");
}
