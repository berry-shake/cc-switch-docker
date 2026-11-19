/**
 * cc-switch-server 的 Web 入口：登录页 + 用量统计 / 会话 / 应用三页。
 *
 * 构建：pnpm build:web（vite.web.config.ts），`@tauri-apps/*` 由 alias 换成 ./tauri-shim 下的 HTTP 实现。
 */
import React from "react";
import ReactDOM from "react-dom/client";
import "../index.css";
import "../i18n";
import { QueryClientProvider } from "@tanstack/react-query";
import { ThemeProvider } from "@/components/theme-provider";
import { TooltipProvider } from "@/components/ui/tooltip";
import { MotionConfig } from "framer-motion";
import { queryClient } from "@/lib/query";
import { Toaster } from "@/components/ui/sonner";
import { FrontendErrorBoundary } from "@/components/FrontendErrorBoundary";
import { initializeInputModality } from "@/lib/inputModality";
import { initializeInterfacePrefs } from "@/lib/interfacePrefs";
import { writeClipboard } from "./tauri-shim/core";
import { WebRoot } from "./WebRoot";

initializeInterfacePrefs();
initializeInputModality();

// 复用的桌面组件直接调 navigator.clipboard；用局域网 IP 走 HTTP 访问时浏览器不提供它，补一个
if (!window.isSecureContext || !navigator.clipboard) {
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: writeClipboard },
  });
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <FrontendErrorBoundary>
      <QueryClientProvider client={queryClient}>
        <ThemeProvider defaultTheme="system" storageKey="cc-switch-theme">
          <MotionConfig reducedMotion="user">
            <TooltipProvider delayDuration={0} skipDelayDuration={0}>
              <WebRoot />
            </TooltipProvider>
          </MotionConfig>
          <Toaster />
        </ThemeProvider>
      </QueryClientProvider>
    </FrontendErrorBoundary>
  </React.StrictMode>,
);
