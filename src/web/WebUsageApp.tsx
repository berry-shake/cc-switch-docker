import { useCallback } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { UsageDashboard } from "@/components/usage/UsageDashboard";
import { useAutoExchangeRates } from "@/hooks/useAutoExchangeRates";
import { useSettingsQuery } from "@/lib/query";
import { settingsApi } from "@/lib/api";
import { toast } from "@/lib/toast";
import type { Settings } from "@/types";

type UsageSettingsPatch = Partial<
  Pick<
    Settings,
    | "usageDisplay"
    | "usageDashboardRefreshIntervalMs"
    | "sessionAutoSyncEnabled"
  >
>;

/**
 * Web 版的用量页外壳：替代桌面版 UsagePage。
 *
 * 桌面版的 useSettings 保存设置时会联动托盘、开机启动、Claude 插件等本机副作用，
 * 这里只读写用量页自己的几项设置，后端 save_settings 也只接受这几项。
 */
export function WebUsageApp() {
  const queryClient = useQueryClient();
  const { data: settings } = useSettingsQuery();

  const save = useCallback(
    async (patch: UsageSettingsPatch) => {
      const current = queryClient.getQueryData<Settings>(["settings"]);
      if (!current) return false;
      const next = { ...current, ...patch } as Settings;
      try {
        await settingsApi.save(next);
        queryClient.setQueryData(["settings"], next);
        return true;
      } catch (error) {
        toast.error(String(error));
        return false;
      }
    },
    [queryClient],
  );

  useAutoExchangeRates(
    settings?.usageDisplay,
    (usageDisplay) => save({ usageDisplay }),
    settings != null,
  );

  return (
    <div className="flex h-screen overflow-hidden bg-app text-fg-1 selection:bg-action/25">
      <main className="relative isolate flex min-w-0 flex-1 flex-col overflow-hidden bg-app">
        <div className="flex min-h-0 flex-1 flex-col">
          <UsageDashboard
            refreshIntervalMs={settings?.usageDashboardRefreshIntervalMs}
            onRefreshIntervalChange={(usageDashboardRefreshIntervalMs) =>
              save({ usageDashboardRefreshIntervalMs })
            }
            sessionAutoSyncEnabled={settings?.sessionAutoSyncEnabled ?? true}
            settingsReady={settings != null}
            onSessionAutoSyncEnabledChange={(sessionAutoSyncEnabled) =>
              save({ sessionAutoSyncEnabled })
            }
            usageDisplay={settings?.usageDisplay}
            onUsageDisplayChange={(usageDisplay) => save({ usageDisplay })}
          />
        </div>
      </main>
    </div>
  );
}
