/**
 * cc-switch-server 的登录会话接口（Cookie 会话，见 src-tauri/src/web_server/auth.rs）。
 */

export interface WebSession {
  authRequired: boolean;
  authenticated: boolean;
  username: string | null;
  version: string;
}

/** invoke / 事件流收到 401 时广播，WebRoot 据此回到登录页 */
export const UNAUTHORIZED_EVENT = "ccs-web:unauthorized";

export function notifyUnauthorized() {
  window.dispatchEvent(new Event(UNAUTHORIZED_EVENT));
}

async function readError(response: Response): Promise<string> {
  const text = await response.text();
  try {
    const value = JSON.parse(text);
    if (typeof value === "string") return value;
  } catch {
    // 不是 JSON，按原文显示
  }
  return text || `${response.status} ${response.statusText}`;
}

export async function fetchSession(): Promise<WebSession> {
  const response = await fetch("api/session", { credentials: "same-origin" });
  if (!response.ok) throw new Error(await readError(response));
  return (await response.json()) as WebSession;
}

export async function login(
  username: string,
  password: string,
  remember: boolean,
): Promise<void> {
  const response = await fetch("api/login", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "same-origin",
    body: JSON.stringify({ username, password, remember }),
  });
  if (!response.ok) throw new Error(await readError(response));
}

export async function logout(): Promise<void> {
  await fetch("api/logout", { method: "POST", credentials: "same-origin" });
}
