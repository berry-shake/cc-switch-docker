import { useCallback } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { settingsApi } from "@/lib/api";
import { toast } from "@/lib/toast";
import type { Settings } from "@/types";

/** Web 版能改的设置；后端 save_settings 也只接受这几项，其余字段原样保留。 */
export type WebSettingsPatch = Partial<
  Pick<
    Settings,
    | "usageDisplay"
    | "usageDashboardRefreshIntervalMs"
    | "sessionAutoSyncEnabled"
    | "visibleApps"
  >
>;

/**
 * 保存 Web 版设置：先改缓存让界面立刻生效，保存失败再把改动的几项改回去。
 *
 * 桌面版的 useSettings 保存时会联动托盘、开机启动、Claude 插件等本机副作用，Web 版不用它。
 */
export function useWebSettingsSave() {
  const queryClient = useQueryClient();

  return useCallback(
    async (patch: WebSettingsPatch) => {
      const current = queryClient.getQueryData<Settings>(["settings"]);
      if (!current) return false;
      const next = { ...current, ...patch } as Settings;
      queryClient.setQueryData(["settings"], next);
      try {
        await settingsApi.save(next);
        return true;
      } catch (error) {
        const revert = Object.fromEntries(
          Object.keys(patch).map((key) => [
            key,
            current[key as keyof WebSettingsPatch],
          ]),
        );
        queryClient.setQueryData<Settings>(["settings"], (latest) =>
          latest ? ({ ...latest, ...revert } as Settings) : latest,
        );
        toast.error(String(error));
        return false;
      }
    },
    [queryClient],
  );
}
