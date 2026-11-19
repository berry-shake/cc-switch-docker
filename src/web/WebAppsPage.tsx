import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { LayoutGrid } from "lucide-react";
import { DEFAULT_VISIBLE_APPS } from "@/config/appConfig";
import { AppPageHeader } from "@/components/shell/AppPageHeader";
import { APP_DISPLAY_NAME, AppGlyph } from "@/components/shell/AppGlyph";
import { Button } from "@/components/ui/button";
import { HelpTip } from "@/components/ui/help-tip";
import { Switch } from "@/components/ui/switch";
import {
  formatTokensCompact,
  getLocaleFromLanguage,
  getResolvedLang,
} from "@/components/usage/format";
import { usageApi } from "@/lib/api";
import { useSettingsQuery } from "@/lib/query";
import { cn } from "@/lib/utils";
import type { VisibleApps } from "@/types";
import { KNOWN_APP_TYPES, type AppType } from "@/types/usage";
import { webText } from "./strings";
import { useWebSettingsSave } from "./useWebSettingsSave";

const ROW_GRID =
  "grid grid-cols-[minmax(0,1fr)_96px_96px_56px] items-center gap-x-4";

interface AppUsage {
  requests: number;
  tokens: number;
}

/**
 * Web 版的「应用」页：选择用量页筛选栏里显示哪些 CLI。
 *
 * 和桌面版一样写 `visibleApps`；服务器上没有安装 / 升级这些本机操作，
 * 换成每个 CLI 的累计请求数和 Token，方便看出哪些真的有数据。
 */
export function WebAppsPage() {
  const { t, i18n } = useTranslation();
  const locale = getLocaleFromLanguage(getResolvedLang(i18n));
  const { data: settings } = useSettingsQuery();
  const save = useWebSettingsSave();

  const usageQuery = useQuery({
    queryKey: ["usage", "web-apps", "summary-by-app"],
    queryFn: () => usageApi.getUsageSummaryByApp(),
    staleTime: 60_000,
  });

  const usageByApp = useMemo(() => {
    const map = new Map<string, AppUsage>();
    for (const { appType, summary } of usageQuery.data ?? []) {
      const prev = map.get(appType) ?? { requests: 0, tokens: 0 };
      map.set(appType, {
        requests: prev.requests + (summary.totalRequests ?? 0),
        tokens: prev.tokens + (summary.realTotalTokens ?? 0),
      });
    }
    return map;
  }, [usageQuery.data]);

  const visibleApps: VisibleApps = {
    ...DEFAULT_VISIBLE_APPS,
    ...settings?.visibleApps,
  };
  // 用量页里 Claude Desktop 显示时 Claude Code 也算显示（两者的用量都记在 claude 下）
  const isVisible = (app: AppType) =>
    visibleApps[app] || (app === "claude" && visibleApps["claude-desktop"]);
  const visibleCount = KNOWN_APP_TYPES.filter(isVisible).length;

  const withVisibility = (pick: (app: AppType) => boolean): VisibleApps => {
    const next = { ...visibleApps };
    for (const app of KNOWN_APP_TYPES) next[app] = pick(app);
    next["claude-desktop"] = next.claude;
    return next;
  };
  const saveVisibleApps = (next: VisibleApps) => {
    if (!settings) return;
    void save({ visibleApps: next });
  };

  const hasData = (app: AppType) => (usageByApp.get(app)?.requests ?? 0) > 0;
  const appsWithData = KNOWN_APP_TYPES.filter(hasData);
  const dataLoaded = usageQuery.isSuccess;
  const onlyWithDataActive =
    dataLoaded &&
    KNOWN_APP_TYPES.every((app) => Boolean(isVisible(app)) === hasData(app));
  const allVisible = visibleCount === KNOWN_APP_TYPES.length;

  // 读到用量后按「有记录 / 暂无记录」分两组，组内沿用筛选栏的顺序
  const sections = dataLoaded
    ? [
        {
          key: "with-data",
          title: webText("sectionWithData", { count: appsWithData.length }),
          apps: appsWithData,
        },
        {
          key: "no-data",
          title: webText("sectionNoData", {
            count: KNOWN_APP_TYPES.length - appsWithData.length,
          }),
          apps: KNOWN_APP_TYPES.filter((app) => !hasData(app)),
        },
      ].filter((section) => section.apps.length > 0)
    : [{ key: "all", title: t("appsPage.columnApp"), apps: KNOWN_APP_TYPES }];

  const numberFormat = new Intl.NumberFormat(locale);

  const renderRow = (app: AppType) => {
    const usage = usageByApp.get(app);
    const checked = Boolean(isVisible(app));
    return (
      <div key={app} className="px-4 py-3">
        <div className={cn(ROW_GRID, "min-h-10")}>
          <div className="flex min-w-0 items-center gap-4">
            <AppGlyph app={app} size={20} badgeClassName="bg-surface" />
            <span className="min-w-0 truncate text-strong text-fg-1">
              {APP_DISPLAY_NAME[app]}
            </span>
          </div>
          <span className="text-end text-body tabular-nums text-fg-2">
            {usage ? numberFormat.format(usage.requests) : "--"}
          </span>
          <span
            className="text-end text-body tabular-nums text-fg-2"
            title={usage ? numberFormat.format(usage.tokens) : undefined}
          >
            {usage ? formatTokensCompact(usage.tokens, locale) : "--"}
          </span>
          <div className="flex justify-end">
            <Switch
              size="sm"
              checked={checked}
              // 至少留一个，筛选栏不至于只剩「全部」
              disabled={!settings || (checked && visibleCount <= 1)}
              onCheckedChange={(value) =>
                saveVisibleApps(
                  withVisibility((other) =>
                    other === app ? value : Boolean(isVisible(other)),
                  ),
                )
              }
              aria-label={webText("showInUsage", {
                name: APP_DISPLAY_NAME[app],
              })}
            />
          </div>
        </div>
      </div>
    );
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden bg-app text-fg-1">
      <AppPageHeader
        icon={<LayoutGrid className="h-5 w-5" strokeWidth={1.5} />}
        title={t("nav.apps")}
        titleExtra={
          <HelpTip title={t("nav.apps")}>{webText("appsHelp")}</HelpTip>
        }
        actions={
          <>
            <Button
              variant="neutral"
              size="regular"
              disabled={
                !settings ||
                !dataLoaded ||
                appsWithData.length === 0 ||
                onlyWithDataActive
              }
              onClick={() => saveVisibleApps(withVisibility(hasData))}
            >
              {webText("showOnlyWithData")}
            </Button>
            <Button
              variant="neutral"
              size="regular"
              disabled={!settings || allVisible}
              onClick={() => saveVisibleApps(withVisibility(() => true))}
            >
              {webText("showAll")}
            </Button>
          </>
        }
      />

      <div className="min-h-0 flex-1 overflow-y-auto scroll-stable">
        <div className="px-6 pb-10 pt-4">
          {usageQuery.isError && (
            <p className="mb-3 text-caption text-danger">
              {webText("loadUsageFailed", {
                message: String(usageQuery.error),
              })}
            </p>
          )}
          <div
            className={cn(
              ROW_GRID,
              "mx-px h-8 px-4 text-caption font-medium text-fg-2",
            )}
          >
            <SectionTitle title={sections[0]?.title} />
            <span className="text-end">{webText("columnRequests")}</span>
            <span className="text-end">{webText("columnTokens")}</span>
            <span className="text-end">{t("appsPage.columnVisible")}</span>
          </div>

          {sections.map((section, index) => (
            <section key={section.key} aria-label={section.title}>
              {index > 0 && (
                <div className="mx-px mt-4 flex h-8 items-center px-4">
                  <SectionTitle title={section.title} />
                </div>
              )}
              <div className="divide-y divide-border overflow-hidden rounded-panel border border-border bg-surface">
                {section.apps.map(renderRow)}
              </div>
            </section>
          ))}
        </div>
      </div>
    </div>
  );
}

function SectionTitle({ title }: { title?: string }) {
  return (
    <h2 className="m-0 truncate text-caption font-semibold text-fg-2">
      {title}
    </h2>
  );
}
