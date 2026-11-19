/**
 * cc-switch-server 的 Web 入口：只渲染用量统计页。
 *
 * 构建：pnpm build:web（vite.web.config.ts），`@tauri-apps/*` 由 alias 换成 ./tauri-shim 下的 HTTP 实现。
 */
import React from "react";
import ReactDOM from "react-dom/client";
import "../index.css";
import { syncLanguageFromSettings } from "../i18n";
import { QueryClientProvider } from "@tanstack/react-query";
import { ThemeProvider } from "@/components/theme-provider";
import { TooltipProvider } from "@/components/ui/tooltip";
import { MotionConfig } from "framer-motion";
import { queryClient } from "@/lib/query";
import { settingsApi } from "@/lib/api";
import { Toaster } from "@/components/ui/sonner";
import { FrontendErrorBoundary } from "@/components/FrontendErrorBoundary";
import { initializeInputModality } from "@/lib/inputModality";
import { initializeInterfacePrefs } from "@/lib/interfacePrefs";
import {
  MODELS_DEV_SYNC_CONFIG_QUERY_KEY,
  syncModelsDevPricingOnStartup,
} from "@/lib/modelsDevAutoSync";
import { WebUsageApp } from "./WebUsageApp";

initializeInterfacePrefs();
initializeInputModality();

void queryClient
  .fetchQuery({ queryKey: ["settings"], queryFn: () => settingsApi.get() })
  .then((settings) => syncLanguageFromSettings(settings.language))
  .catch((error) => console.warn("[web] load settings failed", error));

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <FrontendErrorBoundary>
      <QueryClientProvider client={queryClient}>
        <ThemeProvider defaultTheme="system" storageKey="cc-switch-theme">
          <MotionConfig reducedMotion="user">
            <TooltipProvider delayDuration={0} skipDelayDuration={0}>
              <WebUsageApp />
            </TooltipProvider>
          </MotionConfig>
          <Toaster />
        </ThemeProvider>
      </QueryClientProvider>
    </FrontendErrorBoundary>
  </React.StrictMode>,
);

// 与桌面版一致：开启了 models.dev 自动同步时，打开面板顺带检查定价是否需要更新
void syncModelsDevPricingOnStartup()
  .then((result) => {
    if (!result.skipped) {
      return Promise.all([
        queryClient.invalidateQueries({ queryKey: ["usage"] }),
        queryClient.invalidateQueries({
          queryKey: MODELS_DEV_SYNC_CONFIG_QUERY_KEY,
        }),
      ]);
    }
  })
  .catch((error) => {
    console.warn("[web] models.dev pricing sync failed", error);
    void queryClient.invalidateQueries({
      queryKey: MODELS_DEV_SYNC_CONFIG_QUERY_KEY,
    });
  });
