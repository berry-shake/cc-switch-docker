/**
 * Web 版 `@tauri-apps/api/core`：把 invoke 转成对 cc-switch-server 的 HTTP 调用。
 *
 * 由 vite.web.config.ts 的 alias 注入，桌面版构建不会用到。
 * 请求走相对路径 `api/invoke/<cmd>`，部署在反向代理的子路径下也能用。
 */

import { notifyUnauthorized } from "../auth";

export class WebInvokeError extends Error {}

export async function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const response = await fetch(`api/invoke/${encodeURIComponent(cmd)}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "same-origin",
    body: JSON.stringify(args ?? {}),
  });
  const text = await response.text();
  let data: unknown = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = text;
    }
  }
  if (response.status === 401) {
    // 会话过期或改了密码：通知根组件回到登录页
    notifyUnauthorized();
  }
  if (!response.ok) {
    // Tauri 的 invoke 失败时 reject 的是命令返回的错误值（通常是字符串），这里保持一致
    throw data ?? `${response.status} ${response.statusText}`;
  }
  return data as T;
}

/** 前端里 isTauri() 只用来判断「有没有后端」，Web 版有 HTTP 后端，返回 true */
export function isTauri(): boolean {
  return true;
}

export function convertFileSrc(filePath: string): string {
  return filePath;
}

/** Web 版不支持流式 Channel（只有会话浏览、MCP 检测用到，用量页不涉及） */
export class Channel<T = unknown> {
  onmessage: (response: T) => void = () => {};
  toJSON(): string {
    throw new WebInvokeError("Channel is not supported in the web build");
  }
}
