import { UsageDashboard } from "@/components/usage/UsageDashboard";
import { useAutoExchangeRates } from "@/hooks/useAutoExchangeRates";
import { useSettingsQuery } from "@/lib/query";
import { useWebSettingsSave } from "./useWebSettingsSave";

/**
 * Web 版的用量页外壳：替代桌面版 UsagePage，只读写用量页自己的几项设置。
 */
export function WebUsageApp() {
  const { data: settings } = useSettingsQuery();
  const save = useWebSettingsSave();

  useAutoExchangeRates(
    settings?.usageDisplay,
    (usageDisplay) => save({ usageDisplay }),
    settings != null,
  );

  return (
    <div className="flex min-h-0 flex-1 overflow-hidden bg-app text-fg-1 selection:bg-action/25">
      {/* 和桌面版 App.tsx 一样带 id="content-area"：FullScreenPanel / Sheet 靠它挂进这个 isolate 容器。
          找不到时它们会 fixed 挂到 body 上，z-[60] 压住 z-40 的弹窗，弹窗看不见却锁住了整页点击 */}
      <main
        id="content-area"
        className="relative isolate flex min-w-0 flex-1 flex-col overflow-hidden bg-app"
      >
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
