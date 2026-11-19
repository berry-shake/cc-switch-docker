import { useCallback, useEffect, useRef, useState } from "react";
import {
  ChartColumn,
  History,
  LayoutGrid,
  Loader2,
  LogOut,
  UserRound,
  type LucideIcon,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import ccswitchLogo from "@/assets/icons/logo.svg";
import { Button } from "@/components/ui/button";
import { syncLanguageFromSettings } from "@/i18n";
import { cn } from "@/lib/utils";
import { settingsApi } from "@/lib/api";
import {
  MODELS_DEV_SYNC_CONFIG_QUERY_KEY,
  syncModelsDevPricingOnStartup,
} from "@/lib/modelsDevAutoSync";
import {
  fetchSession,
  logout,
  UNAUTHORIZED_EVENT,
  type WebSession,
} from "./auth";
import { LoginPage } from "./LoginPage";
import { webText } from "./strings";
import { closeEventSource } from "./tauri-shim/event";
import { WebAppsPage } from "./WebAppsPage";
import { WebSessionsPage } from "./WebSessionsPage";
import { WebUsageApp } from "./WebUsageApp";

type RootState =
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "login"; session: WebSession; notice: string | null }
  | { status: "ready"; session: WebSession };

type WebPage = "usage" | "sessions" | "apps";

const PAGES: { id: WebPage; icon: LucideIcon; labelKey: string }[] = [
  { id: "usage", icon: ChartColumn, labelKey: "nav.usage" },
  { id: "sessions", icon: History, labelKey: "nav.sessions" },
  { id: "apps", icon: LayoutGrid, labelKey: "nav.apps" },
];

function pageFromHash(): WebPage {
  const id = window.location.hash.replace(/^#\//, "");
  return PAGES.find((page) => page.id === id)?.id ?? "usage";
}

/** 当前页记在地址的 # 里：刷新后停在原页，浏览器前进后退也能切换。 */
function useHashPage(): WebPage {
  const [page, setPage] = useState(pageFromHash);
  useEffect(() => {
    const handle = () => setPage(pageFromHash());
    window.addEventListener("hashchange", handle);
    return () => window.removeEventListener("hashchange", handle);
  }, []);
  return page;
}

/**
 * Web 面板的根：先问服务端要会话状态，需要登录时显示登录页，登录后渲染用量页。
 * 任何接口返回 401（会话过期、改了密码）都会回到登录页。
 */
export function WebRoot() {
  const queryClient = useQueryClient();
  const [state, setState] = useState<RootState>({ status: "loading" });
  const page = useHashPage();
  const stateRef = useRef(state);
  stateRef.current = state;

  const loadSession = useCallback(async () => {
    setState({ status: "loading" });
    try {
      const session = await fetchSession();
      setState(
        session.authRequired && !session.authenticated
          ? { status: "login", session, notice: null }
          : { status: "ready", session },
      );
    } catch (error) {
      setState({
        status: "error",
        message: error instanceof Error ? error.message : String(error),
      });
    }
  }, []);

  useEffect(() => {
    void loadSession();
  }, [loadSession]);

  // 会话失效：清掉缓存的数据，回到登录页
  useEffect(() => {
    const handle = () => {
      const current = stateRef.current;
      if (current.status !== "ready" || !current.session.authRequired) return;
      closeEventSource();
      queryClient.clear();
      setState({
        status: "login",
        session: { ...current.session, authenticated: false, username: null },
        notice: webText("sessionExpired"),
      });
    };
    window.addEventListener(UNAUTHORIZED_EVENT, handle);
    return () => window.removeEventListener(UNAUTHORIZED_EVENT, handle);
  }, [queryClient]);

  // 进入用量页后：读设置校正语言；开了 models.dev 自动同步时顺带检查定价
  const ready = state.status === "ready";
  useEffect(() => {
    if (!ready) return;
    void queryClient
      .fetchQuery({ queryKey: ["settings"], queryFn: () => settingsApi.get() })
      .then((settings) => syncLanguageFromSettings(settings.language))
      .catch((error) => console.warn("[web] load settings failed", error));
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
  }, [ready, queryClient]);

  const handleLogout = async () => {
    try {
      await logout();
    } finally {
      closeEventSource();
      queryClient.clear();
      await loadSession();
    }
  };

  switch (state.status) {
    case "loading":
      return (
        <div className="flex h-screen items-center justify-center bg-app text-fg-3">
          <Loader2 className="h-5 w-5 animate-spin" />
        </div>
      );
    case "error":
      return (
        <div className="flex h-screen flex-col items-center justify-center gap-3 bg-app text-center">
          <p className="text-strong text-fg-1">{webText("connectFailed")}</p>
          <p className="max-w-md text-caption text-fg-3">{state.message}</p>
          <Button variant="neutral" size="regular" onClick={loadSession}>
            {webText("retry")}
          </Button>
        </div>
      );
    case "login":
      return (
        <LoginPage
          version={state.session.version}
          notice={state.notice}
          onSuccess={() => {
            queryClient.clear();
            void loadSession();
          }}
        />
      );
    case "ready":
      return (
        <div className="flex h-screen flex-col overflow-hidden bg-app text-fg-1">
          <WebTopBar
            session={state.session}
            page={page}
            onLogout={handleLogout}
          />
          <div className="flex min-h-0 flex-1 flex-col">
            {page === "apps" ? (
              <WebAppsPage />
            ) : page === "sessions" ? (
              <WebSessionsPage />
            ) : (
              <WebUsageApp />
            )}
          </div>
        </div>
      );
  }
}

function WebTopBar({
  session,
  page,
  onLogout,
}: {
  session: WebSession;
  page: WebPage;
  onLogout: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex h-10 shrink-0 items-center justify-between border-b border-border bg-sidebar pe-3 ps-6">
      <div className="flex min-w-0 items-center gap-2">
        <img
          src={ccswitchLogo}
          alt=""
          className="h-[18px] w-[18px] select-none"
          draggable={false}
        />
        <span className="text-strong text-fg-1">CC Switch</span>
        <span className="text-caption text-fg-3">v{session.version}</span>
        <nav className="ms-5 flex items-center gap-1" aria-label="CC Switch">
          {PAGES.map(({ id, icon: Icon, labelKey }) => (
            <a
              key={id}
              href={`#/${id}`}
              aria-current={page === id ? "page" : undefined}
              className={cn(
                "flex h-7 items-center gap-1.5 rounded-control px-2.5 text-body text-fg-2 transition-colors hover:bg-subtle hover:text-fg-1",
                page === id &&
                  "bg-selected font-medium text-fg-1 hover:bg-selected",
              )}
            >
              <Icon className="h-4 w-4" strokeWidth={1.75} />
              {t(labelKey)}
            </a>
          ))}
        </nav>
      </div>
      {session.authRequired && (
        <div className="flex items-center gap-1">
          {session.username && (
            <span className="flex items-center gap-1.5 px-2 text-body text-fg-2">
              <UserRound className="h-4 w-4" strokeWidth={1.75} />
              {session.username}
            </span>
          )}
          <Button
            variant="quiet"
            size="compact"
            onClick={onLogout}
            className="gap-1.5 text-fg-2 hover:text-fg-1"
          >
            <LogOut className="h-4 w-4" strokeWidth={1.75} />
            {webText("signOut")}
          </Button>
        </div>
      )}
    </div>
  );
}
