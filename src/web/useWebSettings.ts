import { useCallback } from "react";
import { useSettingsQuery } from "@/lib/query";
import type { Settings } from "@/types";
import {
  useWebSettingsSave,
  type WebSettingsPatch,
} from "./useWebSettingsSave";

/**
 * Web 版的 `@/hooks/useSettings`（vite.web.config.ts 用 alias 换进来）。
 *
 * 桌面版的同名 hook 保存时会联动托盘、开机启动、Claude 插件等本机副作用，
 * Web 面板里复用的桌面组件（会话页）只用到这几个成员：
 * - `updateSettings`：桌面版先改表单草稿；这里保存时已经乐观更新缓存，什么都不用做
 * - `autoSaveSettings`：只保存后端允许写的几项（见 `WebSettingsPatch`），失败时返回 null
 */
export function useSettings() {
  const { data: settings } = useSettingsQuery();
  const save = useWebSettingsSave();

  const updateSettings = useCallback((_updates: Partial<Settings>) => {}, []);

  const autoSaveSettings = useCallback(
    async (updates: WebSettingsPatch) =>
      (await save(updates)) ? { requiresRestart: false } : null,
    [save],
  );

  return {
    settings: settings ?? null,
    isLoading: settings == null,
    updateSettings,
    autoSaveSettings,
  };
}
