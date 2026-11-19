import i18n from "@/i18n";

/**
 * Web 版独有的几条文案。只在 src/web 里用，不进 locales（避免和上游的语言文件冲突）。
 */
const STRINGS = {
  zh: {
    subtitle: "用量统计",
    username: "用户名",
    password: "密码",
    showPassword: "显示密码",
    hidePassword: "隐藏密码",
    remember: "保持登录 30 天",
    signIn: "登录",
    signingIn: "登录中…",
    signOut: "退出登录",
    required: "请输入用户名和密码",
    connectFailed: "无法连接到服务",
    retry: "重试",
    sessionExpired: "登录已过期，请重新登录",
    appsHelp:
      "打开的 CLI 会出现在用量页的筛选栏里。这里只影响显示：隐藏的 CLI 照常采集，也照常计入「全部」。",
    columnRequests: "请求数",
    columnTokens: "Token",
    sectionWithData: "有用量记录（{count}）",
    sectionNoData: "暂无记录（{count}）",
    showOnlyWithData: "只显示有记录的",
    showAll: "全部显示",
    showInUsage: "在用量页显示 {name}",
    loadUsageFailed: "读取用量失败：{message}",
  },
  "zh-TW": {
    subtitle: "用量統計",
    username: "使用者名稱",
    password: "密碼",
    showPassword: "顯示密碼",
    hidePassword: "隱藏密碼",
    remember: "保持登入 30 天",
    signIn: "登入",
    signingIn: "登入中…",
    signOut: "登出",
    required: "請輸入使用者名稱和密碼",
    connectFailed: "無法連線到服務",
    retry: "重試",
    sessionExpired: "登入已過期，請重新登入",
    appsHelp:
      "開啟的 CLI 會出現在用量頁的篩選列。這裡只影響顯示：隱藏的 CLI 照常採集，也照常計入「全部」。",
    columnRequests: "請求數",
    columnTokens: "Token",
    sectionWithData: "有用量記錄（{count}）",
    sectionNoData: "暫無記錄（{count}）",
    showOnlyWithData: "只顯示有記錄的",
    showAll: "全部顯示",
    showInUsage: "在用量頁顯示 {name}",
    loadUsageFailed: "讀取用量失敗：{message}",
  },
  en: {
    subtitle: "Usage Dashboard",
    username: "Username",
    password: "Password",
    showPassword: "Show password",
    hidePassword: "Hide password",
    remember: "Keep me signed in for 30 days",
    signIn: "Sign in",
    signingIn: "Signing in…",
    signOut: "Sign out",
    required: "Enter your username and password",
    connectFailed: "Cannot reach the server",
    retry: "Retry",
    sessionExpired: "Your session has expired. Please sign in again.",
    appsHelp:
      "Enabled CLIs appear in the usage page filter bar. This only affects display: hidden CLIs are still collected and still count toward All.",
    columnRequests: "Requests",
    columnTokens: "Tokens",
    sectionWithData: "With usage ({count})",
    sectionNoData: "No usage yet ({count})",
    showOnlyWithData: "Show only apps with usage",
    showAll: "Show all",
    showInUsage: "Show {name} on the usage page",
    loadUsageFailed: "Failed to load usage: {message}",
  },
} as const;

export type WebStringKey = keyof (typeof STRINGS)["en"];

export function webText(
  key: WebStringKey,
  vars?: Record<string, string | number>,
): string {
  const language = i18n.language ?? "en";
  const text: string =
    language === "zh-TW"
      ? STRINGS["zh-TW"][key]
      : language.startsWith("zh")
        ? STRINGS.zh[key]
        : STRINGS.en[key];
  if (!vars) return text;
  return text.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in vars ? String(vars[name]) : match,
  );
}
