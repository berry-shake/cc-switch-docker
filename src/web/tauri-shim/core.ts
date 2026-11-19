/**
 * Web 版 `@tauri-apps/api/core`：把 invoke 转成对 cc-switch-server 的 HTTP 调用。
 *
 * 由 vite.web.config.ts 的 alias 注入，桌面版构建不会用到。
 * 请求走相对路径 `api/invoke/<cmd>`，部署在反向代理的子路径下也能用。
 *
 * - 参数里带 Channel 的命令（会话正文分块读取）：服务端回 NDJSON，每行转给 Channel 的 onmessage
 * - 服务端回 `application/octet-stream`（会话图片）：返回 ArrayBuffer，同桌面版的 `ipc::Response`
 * - 导出文件、打开外链、写剪贴板：只在浏览器里做，不发请求
 */

import { notifyUnauthorized } from "../auth";

export class WebInvokeError extends Error {}

/** Tauri Channel 的替身：invoke 时从参数里摘出来，服务端的 NDJSON 分块逐个交给 onmessage */
export class Channel<T = unknown> {
  onmessage: (response: T) => void = () => {};
  toJSON(): string {
    throw new WebInvokeError("Channel must be passed directly to invoke()");
  }
}

type Args = Record<string, unknown>;

/** 在浏览器里就能完成的命令 */
const CLIENT_COMMANDS: Record<string, (args: Args) => Promise<unknown>> = {
  // 桌面版弹保存对话框；Web 版直接让浏览器下载
  async export_session_markdown(args) {
    const name = String(args.defaultName || "session.md");
    downloadText(name, String(args.content ?? ""), "text/markdown");
    return name;
  },
  // 桌面版交给系统浏览器；同样只放行 http/https/mailto
  async open_external(args) {
    const url = String(args.url ?? "");
    if (!/^(https?:|mailto:)/i.test(url)) {
      throw `不支持打开该链接: ${url}`;
    }
    window.open(url, "_blank", "noopener,noreferrer");
    return true;
  },
  async copy_text_to_clipboard(args) {
    await writeClipboard(String(args.text ?? ""));
    return true;
  },
};

export async function invoke<T>(cmd: string, args?: Args): Promise<T> {
  const local = CLIENT_COMMANDS[cmd];
  if (local) return (await local(args ?? {})) as T;

  let channel: Channel | null = null;
  const payload: Args = {};
  for (const [key, value] of Object.entries(args ?? {})) {
    if (value instanceof Channel) channel = value;
    else payload[key] = value;
  }

  const response = await fetch(`api/invoke/${encodeURIComponent(cmd)}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "same-origin",
    body: JSON.stringify(payload),
  });
  if (response.status === 401) {
    // 会话过期或改了密码：通知根组件回到登录页
    notifyUnauthorized();
  }
  if (!response.ok) {
    // Tauri 的 invoke 失败时 reject 的是命令返回的错误值（通常是字符串），这里保持一致
    throw (
      (await readBody(response)) ?? `${response.status} ${response.statusText}`
    );
  }
  if (channel) {
    await pumpLines(response, channel);
    return undefined as T;
  }
  const type = response.headers.get("content-type") ?? "";
  if (type.startsWith("application/octet-stream")) {
    return (await response.arrayBuffer()) as T;
  }
  return (await readBody(response)) as T;
}

async function readBody(response: Response): Promise<unknown> {
  const text = await response.text();
  if (!text) return null;
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

/** 逐行读 NDJSON，每行解析后交给 Channel；按行切分，大会话也不必等整份下载完 */
async function pumpLines(response: Response, channel: Channel) {
  const deliver = (line: string) => {
    if (line.trim()) channel.onmessage(JSON.parse(line));
  };
  const reader = response.body?.getReader();
  if (!reader) {
    (await response.text()).split("\n").forEach(deliver);
    return;
  }
  const decoder = new TextDecoder();
  let pending = "";
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    const lines = (pending + decoder.decode(value, { stream: true })).split(
      "\n",
    );
    pending = lines.pop() ?? "";
    lines.forEach(deliver);
  }
  deliver(pending + decoder.decode());
}

function downloadText(name: string, content: string, type: string) {
  const url = URL.createObjectURL(
    new Blob([content], { type: `${type};charset=utf-8` }),
  );
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.style.display = "none";
  document.body.appendChild(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

/**
 * 写剪贴板。`navigator.clipboard` 只在安全上下文（HTTPS / localhost）里有，
 * 用局域网 IP 走 HTTP 访问时退回 execCommand。
 */
export async function writeClipboard(text: string): Promise<void> {
  if (window.isSecureContext && navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }
  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  try {
    if (!document.execCommand("copy")) throw new Error("复制失败");
  } finally {
    textarea.remove();
  }
}

/** 前端里 isTauri() 只用来判断「有没有后端」，Web 版有 HTTP 后端，返回 true */
export function isTauri(): boolean {
  return true;
}

export function convertFileSrc(filePath: string): string {
  return filePath;
}
