import { SessionManagerPage } from "@/components/sessions/SessionManagerPage";

/**
 * Web 版的会话页：直接复用桌面版的 SessionManagerPage，读的是服务器上挂载的会话目录。
 *
 * 删除、多选、在终端恢复、在文件管理器中显示由 `IS_WEB_BUILD` 藏起来；
 * 导出 Markdown 改为浏览器下载（见 tauri-shim/core.ts）。
 */
export function WebSessionsPage() {
  return (
    <div className="flex min-h-0 flex-1 overflow-hidden bg-app text-fg-1 selection:bg-action/25">
      {/* id="content-area" 的用意见 WebUsageApp */}
      <main
        id="content-area"
        className="relative isolate flex min-w-0 flex-1 flex-col overflow-hidden bg-app"
      >
        <SessionManagerPage appId="claude" />
      </main>
    </div>
  );
}
