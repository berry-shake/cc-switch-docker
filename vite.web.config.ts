import path from "node:path";
import { readFileSync } from "node:fs";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

/**
 * cc-switch-server 的 Web 面板构建（只含用量统计页）。
 *
 *   pnpm build:web   → dist-web/
 *
 * `@tauri-apps/*` 全部换成 src/web/tauri-shim 下的实现：invoke 走 HTTP，事件走 SSE。
 * 桌面版构建（vite.config.ts）不受影响。
 */
const shim = (name: string) =>
  path.resolve(__dirname, "src/web/tauri-shim", `${name}.ts`);

const pkg = JSON.parse(
  readFileSync(path.resolve(__dirname, "package.json"), "utf8"),
) as { version: string };

export default defineConfig({
  root: "src",
  plugins: [react()],
  base: "./",
  define: {
    __CC_SWITCH_VERSION__: JSON.stringify(pkg.version),
  },
  build: {
    outDir: "../dist-web",
    emptyOutDir: true,
    rollupOptions: {
      input: {
        index: path.resolve(__dirname, "src/web.html"),
      },
    },
  },
  server: {
    port: 3001,
    strictPort: true,
    proxy: {
      "/api": {
        target: process.env.CC_SWITCH_SERVER_URL ?? "http://127.0.0.1:8765",
        changeOrigin: true,
      },
    },
  },
  resolve: {
    alias: [
      { find: "@tauri-apps/api/core", replacement: shim("core") },
      { find: "@tauri-apps/api/event", replacement: shim("event") },
      { find: "@tauri-apps/api/app", replacement: shim("app") },
      { find: "@tauri-apps/api/path", replacement: shim("path") },
      { find: "@tauri-apps/api/window", replacement: shim("window") },
      { find: "@tauri-apps/api/webview", replacement: shim("webview") },
      { find: "@tauri-apps/plugin-dialog", replacement: shim("plugin-dialog") },
      { find: "@tauri-apps/plugin-log", replacement: shim("plugin-log") },
      {
        find: "@tauri-apps/plugin-process",
        replacement: shim("plugin-process"),
      },
      {
        find: "@tauri-apps/plugin-updater",
        replacement: shim("plugin-updater"),
      },
      { find: "@", replacement: path.resolve(__dirname, "./src") },
    ],
  },
  clearScreen: false,
});
